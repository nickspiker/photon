//! Attachments — send/fetch/save of attachment blobs, wire keys, and the install drain.

use super::*;

/// The send cap (typed attachments Phase 1): the picker still hands the bytes over whole, so RAM at pick time is the bound — 256 MB, up from the 25 MB one-frame limit the chunked wire no longer needs.
pub(super) const MAX_ATTACH: usize = 256 * 1024 * 1024;

/// A RAW's temp copy for limbus (file-only reader): written into the runtime dir under the content hash, None for every other kind or on a write failure. The caller removes it once the decode has landed.
pub(super) fn raw_temp_path(kind: crate::types::AttachKind, hash: &[u8; 32], bytes: &[u8]) -> Option<std::path::PathBuf> {
    if kind != crate::types::AttachKind::RawImage {
        return None;
    }
    let dir = crate::storage::runtime_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("attach-raw-{}.tmp", hex::encode(&hash[..8])));
    match std::fs::write(&path, bytes) {
        Ok(()) => Some(path),
        Err(e) => {
            crate::logf!("attach: RAW temp write failed: {}", e);
            None
        }
    }
}

impl PhotonApp {
    /// Send a dropped/picked file as an attachment (path entry — desktop drop). Reads and forwards to [`Self::send_attachment_from_bytes`].
    pub(super) fn send_attachment_from_path(&mut self, ci: usize, path: &str) {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                crate::logf!("attach: read failed: {}", e);
                return;
            }
        };
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".to_string());
        self.send_attachment_from_bytes(ci, name, bytes);
    }

    /// Byte entry (Android picker + desktop drop converge here). Every file — images included — sends BYTE-EXACT: no re-encode exists in this codebase (house doctrine; the JPEG resample overlay was excised 2026-08-20, the attachments rework is parked).
    pub(super) fn send_attachment_from_bytes(&mut self, ci: usize, name: String, bytes: Vec<u8>) {
        if bytes.is_empty() || bytes.len() > MAX_ATTACH {
            self.ready_toast = Some(tr(Msg::AttachmentLimit).into_owned());
            self.ready_toast_screen = None;
            crate::logf!("attach: rejected ({} bytes)", bytes.len());
            return;
        }
        let Some(peer) = self.contacts.get(ci).map(|c| c.handle_hash) else {
            return;
        };
        // PREPARE OFF-THREAD (typed attachments 2026-09-10): the kind sniff is cheap, but an image's decode for the previews is hundreds of ms on a phone photo and the AV1 encode more — the worker hands back the typed extras and the drain sends the row (re-resolving the contact by handle, the index may have moved). A RAW gets a temp copy for limbus (file-only reader), minted in the worker and removed by the drain.
        let tx = self.attach_prepared_tx.clone();
        queue_job(&self.seal_job_tx, move || {
            let kind = crate::types::sniff(&bytes, &name);
            let raw_tmp = raw_temp_path(kind, blake3::hash(&bytes).as_bytes(), &bytes);
            let p = crate::ui::attach_preview::prepare(&bytes, &name, raw_tmp.as_deref());
            let _ = tx.send(super::AttachPrepared { peer, name, bytes, meta: p.meta, preview: p.preview, blob: p.blob, raw_tmp });
        });
    }

    /// Drain prepared picks: stage the typed extras for the row and send (attach_send_now).
    pub(super) fn drain_attach_prepared(&mut self) {
        while let Ok(p) = self.attach_prepared_rx.try_recv() {
            let Some(ci) = self.contacts.iter().position(|c| c.handle_hash == p.peer) else {
                crate::log("attach: prepared pick has no contact any more — dropped");
                continue;
            };
            if let Some(t) = p.raw_tmp.as_ref() {
                let _ = std::fs::remove_file(t);
            }
            crate::logf!(
                "attach: prepared {} — kind {}{}, micro {} bytes, preview blob {}",
                crate::deglyph_for_log(&p.name),
                format!("{:?}", p.meta.kind),
                p.meta.dims.map_or(String::new(), |(w, h)| format!(", {w}×{h}")),
                p.preview.len(),
                p.blob.as_ref().map_or("none".to_string(), |b| format!("{} bytes", b.len()))
            );
            // The preview blob is stored under its own hash first: the row names it, and it is pushed ahead of the original so the picture lands before the file.
            let mut meta = p.meta;
            if let (Some(blob), Some(ph), Some(seed)) = (p.blob.as_ref(), meta.preview_hash, self.session.as_ref().map(|s| s.identity_seed)) {
                if let Err(e) = crate::storage::blob_store(&seed, &ph, blob) {
                    crate::logf!("attach: preview blob store failed: {}", e);
                    meta.preview_hash = None;
                }
            }
            self.attach_send_now(ci, p.name, p.bytes, meta, p.preview);
        }
    }

    /// The actual send: cap 25MB, blob sealed to disk, the row = an ATTACHMENT_PREFIX content string riding the ordinary chain send (bubble, ACK, fleet sync, tombstones all inherited), then the blob itself pushed over PT.
    pub(super) fn attach_send_now(&mut self, ci: usize, name: String, bytes: Vec<u8>, meta: crate::types::AttachMeta, preview: Vec<u8>) {
        let hash = *blake3::hash(&bytes).as_bytes();
        let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        let manifest = match crate::storage::blob_store_any(&seed, &hash, &bytes) {
            Ok(m) => m,
            Err(e) => {
                crate::logf!("attach: blob store failed: {}", e);
                return;
            }
        };
        let content = crate::types::attachment_content(&hash, &name, bytes.len() as u64);
        // The row: ordinary chain send (or fleet-forward on a chainless device) — everything downstream treats it as a normal message. Its typed extras are STAGED so the minted row carries them before the transmit reads it.
        self.attach_stage = Some((meta, preview));
        if !self.send_chain_message(ci, &content, false, None, None) {
            self.attach_stage = None;
            crate::log("attach: row send failed (no chain, no fleet) — attachment stays local");
        }
        // The preview blob first (small, the picture the friend sees before the file lands), then the original: one frame for a small file, manifest + chunks for a large one. Siblings + offline races fetch on demand (attach_req).
        if let Some(ph) = meta.preview_hash {
            self.send_attach_blob(ci, &ph);
        }
        match manifest {
            Some(m) => self.send_attach_chunks(ci, &hash, m),
            None => self.send_attach_blob(ci, &hash),
        }
        self.msg_wrap = None;
        self.scene_dirty = true;
        crate::logf!(
            "attach: sent {} ({} bytes)",
            crate::deglyph_for_log(&name),
            bytes.len()
        );
    }

    /// The wire seal key for an attachment exchanged with `device`: fleet key when the device is one of OUR siblings, else the conversation's friendship history key. (Blob files at rest use a separate local-only key.)
    pub(super) fn attach_wire_key(&self, device: &[u8; 32], token: &[u8; 32]) -> Option<[u8; 32]> {
        let is_sib = self
            .contacts
            .iter()
            .any(|c| c.is_sibling && c.device_key() == Some(*device));
        if is_sib {
            return self.fleet_key_cached();
        }
        self.friendship_chains
            .iter()
            .find(|(_, ch)| ch.conversation_token == *token)
            .and_then(|(_, ch)| ch.history_key().copied())
    }

    /// Seal + push the blob for `content_hash` to contact `ci`'s device over PT (skips self/sibling contacts — they fetch lazily).
    pub(super) fn send_attach_blob(&mut self, ci: usize, content_hash: &[u8; 32]) {
        let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        let Some(plain) = crate::storage::blob_load(&seed, content_hash) else {
            crate::log("attach: blob missing locally — nothing to push");
            return;
        };
        let (device, addr_pair, relay_to, token) = {
            let Some(c) = self.contacts.get(ci) else {
                return;
            };
            // Nobody to push a blob to: our own notes have zero remote participants, and a sibling is our own fleet (it reads the blob from the fleet store, not from us).
            if self
                .our_party_id(c)
                .is_none_or(|us| c.remote_count(&us) == 0)
                || c.is_sibling
            {
                return;
            }
            let Some(token) = self
                .friendship_chains
                .iter()
                .find(|(id, _)| Some(*id) == c.friendship_id)
                .map(|(_, ch)| ch.conversation_token)
            else {
                crate::log("attach: no chains yet — blob waits for attach_req");
                return;
            };
            let relay_to = relay_unless_direct_trusted(&c, crate::network::udp::get_local_ip());
            let Some(recipient_key) = c.device_key() else {
                return; // nowhere to send a blob without a known device
            };
            (recipient_key, c.race_addrs(), relay_to, token)
        };
        let Some((peer_addr, alt_addr)) = addr_pair else {
            return;
        };
        let Some(wire_key) = self.attach_wire_key(&device, &token) else {
            crate::log("attach: no wire key (history key not derived yet)");
            return;
        };
        let Ok(sealed) = kete::encrypt_bytes(&plain, &wire_key) else {
            return;
        };
        let (Some(kp), Some(checker)) =
            (self.device_keypair.as_ref(), self.status_checker.as_ref())
        else {
            return;
        };
        match crate::network::fgtw::protocol::build_attach_blob_vsf(
            &token,
            content_hash,
            sealed,
            kp.public.as_bytes(),
            kp.secret.as_bytes(),
        ) {
            Ok(vsf_bytes) => {
                checker.send_history(crate::network::status::HistorySendRequest {
                    peer_addr,
                    alt_addr,
                    recipient_pubkey: device,
                    vsf_bytes,
                    relay_to,
                });
                crate::log("attach: blob dispatched over PT");
            }
            Err(e) => crate::logf!("attach: blob frame build failed: {}", e),
        }
    }

    /// Push a CHUNKED blob to the friend: the sealed manifest, then every chunk, each its own PT frame — sealed and dispatched from the worker (256 seals of 256 KB do not belong on the render thread).
    pub(super) fn send_attach_chunks(&mut self, ci: usize, content_hash: &[u8; 32], m: crate::storage::BlobManifest) {
        let (device, addr_pair, relay_to, token) = {
            let Some(c) = self.contacts.get(ci) else {
                return;
            };
            if self.our_party_id(c).is_none_or(|us| c.remote_count(&us) == 0) || c.is_sibling {
                return;
            }
            let Some(token) = self
                .friendship_chains
                .iter()
                .find(|(id, _)| Some(*id) == c.friendship_id)
                .map(|(_, ch)| ch.conversation_token)
            else {
                crate::log("attach: no chains yet — chunks wait for attach_req");
                return;
            };
            let relay_to = relay_unless_direct_trusted(&c, crate::network::udp::get_local_ip());
            let Some(recipient_key) = c.device_key() else {
                return;
            };
            (recipient_key, c.race_addrs(), relay_to, token)
        };
        let Some((peer_addr, alt_addr)) = addr_pair else {
            return;
        };
        let Some(wire_key) = self.attach_wire_key(&device, &token) else {
            crate::log("attach: no wire key (history key not derived yet)");
            return;
        };
        let (Some(kp), Some(checker)) = (self.device_keypair.as_ref(), self.status_checker.as_ref()) else {
            return;
        };
        let (kp_pub, kp_sec) = (*kp.public.as_bytes(), *kp.secret.as_bytes());
        let dispatch = checker.history_dispatch();
        let content_hash = *content_hash;
        queue_job(&self.seal_job_tx, move || {
            let send = |vsf_bytes: Vec<u8>| {
                let _ = dispatch.send(crate::network::status::HistorySendRequest {
                    peer_addr,
                    alt_addr,
                    recipient_pubkey: device,
                    vsf_bytes,
                    relay_to: relay_to.clone(),
                });
            };
            match kete::encrypt_bytes(&m.to_bytes(), &wire_key).and_then(|sealed| {
                crate::network::fgtw::protocol::build_attach_manifest_vsf(&token, &content_hash, sealed, &kp_pub, &kp_sec)
            }) {
                Ok(v) => send(v),
                Err(e) => {
                    crate::logf!("attach: manifest frame build failed: {}", e);
                    return;
                }
            }
            let mut sent = 0usize;
            for (i, h) in m.chunks.iter().enumerate() {
                let Some(plain) = crate::storage::blob_chunk_load(h) else {
                    crate::logf!("attach: chunk {} missing locally — skipped", i);
                    continue;
                };
                match kete::encrypt_bytes(&plain, &wire_key).and_then(|sealed| {
                    crate::network::fgtw::protocol::build_attach_chunk_vsf(&token, &content_hash, i as u32, sealed, &kp_pub, &kp_sec)
                }) {
                    Ok(v) => {
                        send(v);
                        sent += 1;
                    }
                    Err(e) => crate::logf!("attach: chunk {} frame build failed: {}", i, e),
                }
            }
            crate::logf!("attach: manifest + {} of {} chunk(s) dispatched over PT", sent, m.chunks.len());
        });
    }

    /// Ask for a missing blob: attach_req to the conversation's friend device AND every online sibling — whoever holds it answers with an attach_blob (or, for a chunked blob, the manifest + chunks; a held manifest turns the ask into a RESUME carrying the want bitmap of the chunks still missing).
    pub(super) fn attach_fetch(&mut self, sci: usize, content_hash: &[u8; 32]) {
        // Token: the friendship token when one exists; else (self-conversation — no chains) the handle hash. Sibling exchanges seal under the FLEET key regardless of token, so the fallback only ever reaches sibling responders, where it's a plain discriminator.
        let Some(token) = ({
            let c = self.contacts.get(sci);
            c.map(|c| {
                self.friendship_chains
                    .iter()
                    .find(|(id, _)| Some(*id) == c.friendship_id)
                    .map(|(_, ch)| ch.conversation_token)
                    .unwrap_or(c.handle_hash)
            })
        }) else {
            return;
        };
        let (Some(kp), Some(checker)) =
            (self.device_keypair.as_ref(), self.status_checker.as_ref())
        else {
            return;
        };
        // Resume: the manifest is here and some chunks are not → ask only for those.
        let want: Option<Vec<u8>> = crate::storage::blob_chunks_held(content_hash)
            .filter(|held| held.iter().any(|h| !h))
            .map(|held| crate::storage::BlobManifest::want_bitmap(&held));
        if let Some(w) = want.as_ref() {
            let missing = w.iter().map(|b| b.count_ones()).sum::<u32>();
            crate::logf!("attach: resuming — {} chunk(s) still wanted", missing);
        }
        let Ok(vsf_bytes) = crate::network::fgtw::protocol::build_attach_req_want_vsf(
            &token,
            content_hash,
            want.as_deref(),
            kp.public.as_bytes(),
            kp.secret.as_bytes(),
        ) else {
            return;
        };
        // Friend device + all siblings; race_addrs handles LAN/WAN, relay list covers the unreachable.
        let mut targets: Vec<(
            std::net::SocketAddr,
            Option<std::net::SocketAddr>,
            [u8; 32],
            Vec<[u8; 32]>,
        )> = Vec::new();
        for c in &self.contacts {
            let is_target = c.is_sibling
                || self.contacts.get(sci).map(|t| t.handle_hash) == Some(c.handle_hash);
            if !is_target {
                continue;
            }
            if let Some((a, alt)) = c.race_addrs() {
                let relay = relay_unless_direct_trusted(&c, crate::network::udp::get_local_ip());
                if let Some(k) = c.device_key() {
                    targets.push((a, alt, k, relay));
                }
            }
        }
        for (peer_addr, alt_addr, recipient_pubkey, relay_to) in targets {
            checker.send_history(crate::network::status::HistorySendRequest {
                peer_addr,
                alt_addr,
                recipient_pubkey,
                vsf_bytes: vsf_bytes.clone(),
                relay_to,
            });
        }
        crate::log("attach: fetch request dispatched");
    }

    /// Save a held blob to the user's Downloads dir (name deduped). Returns the destination on success.
    pub(super) fn attach_save(&mut self, name: &str, content_hash: &[u8; 32]) -> Option<String> {
        let seed = self.session.as_ref().map(|s| s.identity_seed)?;
        #[cfg(target_os = "android")]
        let base = crate::storage::photon_config_dir().ok()?.join("Download");
        #[cfg(not(target_os = "android"))]
        let base = dirs::download_dir()?;
        let _ = std::fs::create_dir_all(&base);
        // Dedupe: name, name (2), name (3)…
        let mut dest = base.join(name);
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), format!(".{}", e)),
            None => (name.to_string(), String::new()),
        };
        let mut i = 2;
        while dest.exists() {
            dest = base.join(format!("{} ({}){}", stem, i, ext));
            i += 1;
        }
        // Streams chunk by chunk with a running hash — a chunked blob is never rebuilt in RAM.
        crate::storage::blob_write_file(&seed, content_hash, &dest)?;
        Some(dest.to_string_lossy().into_owned())
    }

    /// Drain completed peer-avatar downloads: colour-convert the VSF-RGB pixels to the display buffer (same path as the self avatar) and install them on the matching contact, invalidating its scaled cache so the next render rebuilds + shows it. A `None` result (no avatar / fetch failed) just leaves the placeholder.
    /// Drain attachment blobs a worker verified + stored off-thread: send the attach_have confirm (needs the keypair + checker, which is why it can't run in the worker) so the pusher's pill flips to delivered, then clear the compose wrap and repaint.
    pub(super) fn drain_attach_installed(&mut self) {
        while let Ok(r) = self.attach_installed_rx.try_recv() {
            // A chunk: count it toward the bar; only the LAST one is an install (attach_have + re-sniff below).
            let mut complete = true;
            if let Some((idx, total, done)) = r.chunk {
                let e = self.attach_chunk_progress.entry(r.content_hash).or_insert((0, total));
                e.0 = (e.0 + 1).min(total);
                e.1 = total;
                complete = done;
                if done {
                    crate::logf!("ATTACH: chunked blob complete — {} chunk(s)", total);
                    self.attach_chunk_progress.remove(&r.content_hash);
                } else if idx % 16 == 0 {
                    crate::logf!("ATTACH: chunk {} of {} stored", idx + 1, total);
                }
                self.scene_dirty = true;
            } else {
                crate::logf!("ATTACH: blob received + stored ({} bytes)", r.len);
            }
            // RE-SNIFF (the receiver's own verdict): the row's kind is the peer's claim until the bytes are here; a stricter local sniff wins (a program dressed as a picture reads as a program from now on).
            if let Some(local) = r.sniffed {
                for conv in self.conversations.iter_mut() {
                    for m in conv.messages.iter_mut() {
                        let is_row = crate::types::parse_attachment_content(&m.content).is_some_and(|(h, _, _)| h == r.content_hash);
                        if !is_row {
                            continue;
                        }
                        let claimed = m.attach.map_or(crate::types::AttachKind::Unknown, |a| a.kind);
                        let verdict = crate::types::AttachKind::reconcile(local, claimed);
                        if verdict != claimed {
                            crate::logf!("ATTACH: kind reconciled {} → {} (our sniff outranks the claim)", format!("{claimed:?}"), format!("{verdict:?}"));
                            let mut a = m.attach.unwrap_or(crate::types::AttachMeta { kind: verdict, dims: None, preview_hash: None });
                            a.kind = verdict;
                            m.attach = Some(a);
                        }
                    }
                }
            }
            if !complete {
                continue;
            }
            if let (Some(kp), Some(checker)) =
                (self.device_keypair.as_ref(), self.status_checker.as_ref())
            {
                if let Ok(vsf_bytes) = crate::network::fgtw::protocol::build_attach_have_vsf(
                    &r.conversation_token,
                    &r.content_hash,
                    kp.public.as_bytes(),
                    kp.secret.as_bytes(),
                ) {
                    checker.send_history(crate::network::status::HistorySendRequest {
                        peer_addr: r.sender_addr,
                        alt_addr: None,
                        recipient_pubkey: r.sender_pubkey.key,
                        vsf_bytes,
                        relay_to: vec![r.sender_pubkey.key], // always the one-device relay copy — responses die on one-directional reverse paths
                    });
                }
            }
            self.msg_wrap = None;
            self.scene_dirty = true;
        }
    }
}
