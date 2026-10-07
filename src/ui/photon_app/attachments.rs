//! Attachments — send/fetch/save of attachment blobs, wire keys, and the install drain.

use super::*;

/// The send cap (typed attachments Phase 1): the picker still hands the bytes over whole, so RAM at pick time is the bound — 256 MB, up from the 25 MB one-frame limit the chunked wire no longer needs.
pub(super) const MAX_ATTACH: usize = 256 * 1024 * 1024;

/// A RAW's temp copy for limbus (file-only reader): written into the runtime dir under the content hash, None for every other kind or on a write failure. The caller removes it once the decode has landed.
/// A temp copy of any picked/held image for the opsin viewer path (its readers want a path). opsin recognises the file by its bytes, so the extension is only for the human who sees the name in the viewer's title; it is the original's when there is one, else what the bytes' magic says. Lives in runtime_dir beside the RAW temp; the caller removes it after the render.
pub(super) fn view_temp_path(name: &str, hash: &[u8; 32], bytes: &[u8]) -> Option<std::path::PathBuf> {
    // The name's extension when it has one; images travel nameless now, so the bytes' own magic names the rest.
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let ext = if ext.is_empty() { crate::types::sniff_ext(bytes).to_string() } else { ext };
    let dir = crate::storage::runtime_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("attach-view-{}.{}", hex::encode(&hash[..8]), if ext.is_empty() { "bin" } else { ext.as_str() }));
    match std::fs::write(&path, bytes) {
        Ok(()) => Some(path),
        Err(e) => {
            crate::logf!("attach: view temp write failed: {}", e);
            None
        }
    }
}



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
    /// The paperclip pressed: Android raises the any-file picker request the Choreographer poll drains into Kotlin; desktop has no picker dependency, so it says where to drop the file.
    pub(super) fn compose_attach_click(&mut self) {
        if !matches!(self.state, AppState::Conversation) {
            return;
        }
        #[cfg(target_os = "android")]
        {
            self.pending_attach_picker = true;
            crate::log("attach: picker requested");
        }
        #[cfg(not(target_os = "android"))]
        {
            self.ready_toast = Some(tr(Msg::AttachDropHint).into_owned());
            self.ready_toast_screen = None;
        }
        self.scene_dirty = true;
    }

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
        let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        let tx = self.attach_prepared_tx.clone();
        queue_job(&self.seal_job_tx, move || {
            let kind = crate::types::sniff(&bytes, &name);
            let hash = *blake3::hash(&bytes).as_bytes();
            let raw_tmp = raw_temp_path(kind, &hash, &bytes);
            let p = crate::ui::attach_preview::prepare(&bytes, &name, raw_tmp.as_deref());
            // Sealed to disk HERE, not in the drain: the row and the push need only the manifest.
            let manifest = match crate::storage::blob_store_any(&seed, &hash, &bytes) {
                Ok(m) => m,
                Err(e) => {
                    crate::logf!("attach: blob store failed: {}", e);
                    if let Some(t) = raw_tmp.as_ref() {
                        let _ = std::fs::remove_file(t);
                    }
                    return;
                }
            };
            let _ = tx.send(super::AttachPrepared { peer, name, bytes, meta: p.meta, preview: p.preview, blob: p.blob, raw_tmp, hash, manifest });
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
            self.attach_send_now(ci, p.name, p.bytes, meta, p.preview, p.hash, p.manifest);
        }
    }

    /// The actual send: cap 25MB, blob sealed to disk, the row = an ATTACHMENT_PREFIX content string riding the ordinary chain send (bubble, ACK, fleet sync, tombstones all inherited), then the blob itself pushed over PT.
    pub(super) fn attach_send_now(&mut self, ci: usize, name: String, bytes: Vec<u8>, meta: crate::types::AttachMeta, preview: Vec<u8>, hash: [u8; 32], manifest: Option<crate::storage::BlobManifest>) {
        // Images and music travel NAMELESS (Nick: "the user typed nothing. just an image" / "no name, just the waveform") — a filename is metadata nobody chose to send; the receiver derives extensions from the bytes' own magic.
        let wire_name = if meta.kind.is_image() || meta.kind == crate::types::AttachKind::Audio { "" } else { name.as_str() };
        let file = crate::types::AttachRef::file(hash, wire_name, bytes.len() as u64);
        // The row: ordinary chain send (or fleet-forward on a chainless device) — everything downstream treats it as a normal message. Its typed extras are STAGED so the minted row carries them before the transmit reads it.
        self.attach_stage = Some((meta, preview, file));
        if !self.send_chain_message(ci, "", false, None, None) {
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
                    tag: Some(*content_hash), // the bar for THIS blob reads its transfer by this tag
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
        self.attach_send_total.insert(content_hash, m.chunks.len() as u32);
        queue_job(&self.seal_job_tx, move || {
            // Only the CHUNKS carry the blob's tag: the bar counts them against the chunk total, and the manifest is not one of them.
            let send = |vsf_bytes: Vec<u8>, tag: Option<[u8; 32]>| {
                let _ = dispatch.send(crate::network::status::HistorySendRequest {
                    peer_addr,
                    alt_addr,
                    recipient_pubkey: device,
                    vsf_bytes,
                    relay_to: relay_to.clone(),
                    tag,
                });
            };
            match kete::encrypt_bytes(&m.to_bytes(), &wire_key).and_then(|sealed| {
                crate::network::fgtw::protocol::build_attach_manifest_vsf(&token, &content_hash, sealed, &kp_pub, &kp_sec)
            }) {
                Ok(v) => send(v, None),
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
                        send(v, Some(content_hash));
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
        // ONE DEVICE PER ASK (field 2026-09-17, Nick: "re-uploads/replications of pigeons"): the request used to fan out to the friend's every device AND all our siblings at once, and every holder answered with the whole blob — a 142 MB pigeon served in full by three devices, ~300 KB pigeons landing nine times each. Now the candidates are ranked — the friend's devices with a proven direct path first, then the friend's other online devices, then our online siblings, then the rest — one is asked, and each 20 s re-ask (attach_fetch_retry_tick) moves to the next.
        let tries = self.attach_fetch_inflight.get(content_hash).map_or(0, |(_, _, n)| *n);
        let mut targets: Vec<(
            u8,
            std::net::SocketAddr,
            Option<std::net::SocketAddr>,
            [u8; 32],
            Vec<[u8; 32]>,
        )> = Vec::new();
        for c in &self.contacts {
            let is_friend = self.contacts.get(sci).map(|t| t.handle_hash) == Some(c.handle_hash);
            let is_target = c.is_sibling || is_friend;
            if !is_target {
                continue;
            }
            let primary = c.device_key();
            if let Some((a, alt)) = c.race_addrs() {
                let relay = relay_unless_direct_trusted(&c, crate::network::udp::get_local_ip());
                if let Some(k) = primary {
                    let rank = match (is_friend, c.is_online, c.validated_path.is_some()) {
                        (true, true, true) => 0,
                        (true, true, false) => 1,
                        (false, true, _) => 2,
                        (true, false, _) => 3,
                        (false, false, _) => 4,
                    };
                    targets.push((rank, a, alt, k, relay));
                }
            }
            // EVERY DEVICE OF THE FRIEND (field 2026-10-07, Jeff's PDF): the contact row carries ONE device's addresses — the one in their hand — so a file whose only copy sits on the sender's OTHER device (the desktop it was sent from) was never asked for; every retry went to the same phone. Each other known device of the friend joins the list at its own address with its own relay leg; the retry tick still asks ONE device per round, rotating, so this widens the reach without fanning out.
            if is_friend {
                for ep in &c.device_endpoints {
                    if Some(ep.pubkey) == primary || c.refused_devices.contains(&ep.pubkey) {
                        continue;
                    }
                    let Some(addr) = ep.lan.or(ep.public) else { continue };
                    let alt = if ep.lan.is_some() { ep.public } else { None };
                    targets.push((if ep.online { 1 } else { 3 }, addr, alt, ep.pubkey, vec![ep.pubkey]));
                }
            }
        }
        targets.sort_by_key(|t| t.0);
        if targets.is_empty() {
            crate::log("attach: fetch has nobody to ask — no device of the friend or ours has an address");
            return;
        }
        let (rank, peer_addr, alt_addr, recipient_pubkey, relay_to) = targets[tries as usize % targets.len()].clone();
        checker.send_history(crate::network::status::HistorySendRequest {
            peer_addr,
            alt_addr,
            recipient_pubkey,
            vsf_bytes: vsf_bytes.clone(),
            relay_to,
            tag: None,
        });
        crate::logf!("attach: fetch request dispatched to device {} (rank {}, candidate {} of {})", crate::fp(&recipient_pubkey), rank, tries as usize % targets.len() + 1, targets.len());
        let Some(conv_id) = self.contacts.get(sci).map(|c| c.id) else { return };
        self.attach_fetch_inflight.insert(*content_hash, (conv_id, std::time::Instant::now(), tries.saturating_add(1))); // WHY/PROOF: a u8 of fetch rounds for a blob no device answers — it outlives 255, and saturating keeps the candidate rotation from wrapping back to rank 0's first try
    }

    /// THE FLEET HOLDS EVERYTHING (Nick 2026-10-07: "if I have five devices, all five should get copies of waves, pigeons, beams, photons, all of it… be sane"). One pass per edge (`replicate_dirty`: rows arrived, presence answers landed, the network went unmetered): every live file row in every conversation whose bytes this device is KNOWN not to hold — pigeons, wave recordings, wave envelopes, previews' originals — is armed ONCE per session in the fetch map, and the retry tick asks one device at a time (friend's devices and our siblings, easiest first) until it lands. Arming instead of asking at once leaves a fresh row's own push its window. A row whose presence is not known yet queues its probe here and is picked up on the edge its answer raises. Sane: a metered network arms nothing (Android reports it; desktops are never metered here).
    pub(super) fn replicate_sweep(&mut self) {
        let rows: usize = self.conversations.iter().map(|c| c.messages.len()).sum();
        if rows != self.replicate_rows_seen {
            self.replicate_rows_seen = rows;
            self.replicate_dirty = true;
        }
        let metered = crate::network::wfd::net_metered();
        if metered != self.replicate_metered {
            self.replicate_metered = metered;
            self.replicate_dirty |= !metered;
        }
        if !std::mem::take(&mut self.replicate_dirty) || metered || self.session.is_none() {
            return;
        }
        let now = std::time::Instant::now();
        let mut armed = 0usize;
        for ci in 0..self.contacts.len() {
            if self.contacts[ci].is_sibling {
                continue;
            }
            let cid = self.contacts[ci].id;
            let hashes: Vec<[u8; 32]> = self
                .conv_of(ci)
                // A device that opted out of holding waves (`waves.hold` off — a watch) skips recordings; everything else is held.
                .map(|v| v.messages.iter().filter(|m| !m.deleted && (self.wave_hold || !m.is_wave_recording())).filter_map(|m| m.file_parts().map(|(h, _, _)| h)).collect())
                .unwrap_or_default();
            for h in hashes {
                if self.attach_auto_fetched.contains(&h) || self.attach_fetch_inflight.contains_key(&h) {
                    continue;
                }
                if crate::storage::blob_present_known(&h) == Some(false) {
                    self.attach_auto_fetched.insert(h);
                    self.attach_fetch_inflight.insert(h, (cid, now, 0));
                    armed += 1;
                }
            }
        }
        if armed > 0 {
            crate::logf!("REPLICATE: {} file(s) not on this device — fetching each from the fleet, one device at a time", armed);
        }
    }

    /// Re-ask for fetches nobody answered: every 20 s while nothing has landed (no blob, no manifest), up to eight times, then let go. Landed fetches leave the map at once.
    pub(super) fn attach_fetch_retry_tick(&mut self) {
        const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(20);
        const MAX_TRIES: u8 = 8;
        let now = std::time::Instant::now();
        let due: Vec<([u8; 32], ContactId, u8)> = self
            .attach_fetch_inflight
            .iter()
            .filter(|(_, (_, at, _))| now.duration_since(*at) >= RETRY_AFTER)
            .map(|(h, (id, _, n))| (*h, *id, *n))
            .collect();
        for (hash, id, tries) in due {
            if crate::storage::blob_present(&hash) || crate::storage::blob_manifest(&hash).is_some() {
                self.attach_fetch_inflight.remove(&hash);
                continue;
            }
            // The conversation's contact may be gone since the ask — its fetch is let go, never re-aimed at whoever took its row.
            let Some(sci) = self.ci_of(&id).filter(|_| tries < MAX_TRIES) else {
                crate::logf!("attach: fetch of {}… let go after {} asks — nobody answered, or its contact was removed", hex::encode(&hash[..4]), tries);
                self.attach_fetch_inflight.remove(&hash);
                continue;
            };
            if tries == 0 {
                crate::logf!("attach: replicating {}… — first ask", hex::encode(&hash[..4]));
            } else {
                crate::logf!("attach: fetch of {}… unanswered for {}s — asking again ({} of {})", hex::encode(&hash[..4]), RETRY_AFTER.as_secs(), tries + 1, MAX_TRIES);
            }
            self.attach_fetch(sci, &hash);
        }
    }

    /// EXPORT a kept wave in `fmt` (wave/export.rs): the vault read and the decode run on a worker — an hour of audio decodes for seconds and the UI thread never touches the vault — then the file goes where Save puts files (Downloads; on Android the public Downloads/Photon). The result toasts from the tick drain.
    pub(super) fn wave_export_start(&mut self, sci: usize, ts: i64, fmt: crate::wave::export::ExportFormat) {
        let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) else { return };
        let rec = self.conv_of(sci).and_then(|v| {
            v.messages
                .iter()
                .filter(|m| !m.deleted && matches!(m.reference, Some((crate::types::RefKind::Wave, t)) if t == ts))
                .find(|m| m.is_wave_recording())
                .and_then(|m| m.file_parts().map(|(h, _, _)| h))
        });
        let Some(hash) = rec else { return };
        let who = self.contacts.get(sci).map(|c| c.display_name()).unwrap_or_default();
        #[cfg(target_os = "android")]
        let Some(dir) = crate::storage::photon_config_dir().ok().map(|d| d.join("staging")) else { return };
        #[cfg(not(target_os = "android"))]
        let Some(dir) = dirs::download_dir() else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = self.event_proxy.clone();
        self.ready_toast = Some(tr(Msg::ExportingWave).into_owned());
        let _ = std::thread::Builder::new().name("wave-export".into()).spawn(move || {
            let out = (|| {
                let blob = crate::storage::blob_load(&seed, &hash)?;
                let _ = std::fs::create_dir_all(&dir);
                let start = blob.get(13..21).and_then(|b| b.try_into().ok()).map(i64::from_le_bytes).unwrap_or(ts);
                let name = crate::wave::export::file_name(start, &who, fmt);
                let path = dir.join(&name);
                // The one sanctioned Downloads writer opens the file; the export only fills it.
                let mut file = std::fs::File::create(&path).ok()?;
                match crate::wave::export::export_to(&blob, fmt, &mut file) {
                    Ok(frames) => crate::logf!("WAVE: exported {} ({} frames, {})", name, frames, fmt.label()),
                    Err(e) => {
                        crate::logf!("WAVE: export failed — {}", e);
                        let _ = std::fs::remove_file(&path);
                        return None;
                    }
                }
                #[cfg(target_os = "android")]
                {
                    let shared = crate::platform::jni_android::save_to_downloads(&path, &name);
                    let _ = std::fs::remove_file(&path); // the staging copy is done with either way
                    shared
                }
                #[cfg(not(target_os = "android"))]
                Some(path.to_string_lossy().into_owned())
            })();
            let _ = tx.send(out);
            #[cfg(not(target_os = "android"))]
            if let Some(w) = wake.as_ref() {
                let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
            }
            #[cfg(target_os = "android")]
            let _ = wake;
        });
        self.wave_export_rx = Some(rx);
    }

    /// A finished export: toast where it went (or that it failed). Returns true when something landed.
    pub(super) fn drain_wave_export(&mut self) -> bool {
        let Some(res) = self.wave_export_rx.as_ref().and_then(|rx| rx.try_recv().ok()) else { return false };
        self.wave_export_rx = None;
        self.ready_toast = Some(match res {
            Some(dest) => tr(Msg::SavedTo(&dest)).into_owned(),
            None => tr(Msg::SaveFailed).into_owned(),
        });
        self.ready_toast_screen = None;
        true
    }

    /// Save a held blob to the user's Downloads dir under its own name, replacing an earlier save of that name. Returns the destination on success.
    pub(super) fn attach_save(&mut self, name: &str, content_hash: &[u8; 32]) -> Option<String> {
        let seed = self.session.as_ref().map(|s| s.identity_seed)?;
        // Android: stage in the app's private dir, then hand the file to the PUBLIC Downloads/Photon (MediaStore) — the private dir was invisible to the user and every other app (Nick 2026-09-28: "somewhere sane and shared").
        #[cfg(target_os = "android")]
        let base = crate::storage::photon_config_dir().ok()?.join("staging");
        #[cfg(not(target_os = "android"))]
        let base = dirs::download_dir()?;
        let _ = std::fs::create_dir_all(&base);
        // A nameless attachment (images travel without filenames) saves under its hash prefix with a magic-sniffed extension.
        let derived;
        let name = if name.is_empty() {
            let head = crate::storage::blob_load(&seed, content_hash).unwrap_or_default();
            derived = format!("photon-{}.{}", hex::encode(&content_hash[..8]), crate::types::sniff_ext(&head));
            derived.as_str()
        } else {
            name
        };
        // Stream the blob under exactly that name, replacing any earlier save — the same landing a bridge pigeon does, factored out.
        let landed = crate::storage::land_blob(&seed, content_hash, &base, name)?;
        #[cfg(target_os = "android")]
        {
            let staged = std::path::PathBuf::from(&landed);
            let shared = crate::platform::jni_android::save_to_downloads(&staged, name);
            let _ = std::fs::remove_file(&staged); // the staging copy is done with either way
            return shared;
        }
        #[cfg(not(target_os = "android"))]
        Some(landed)
    }

    /// Drain completed peer-avatar downloads: colour-convert the VSF-RGB pixels to the display buffer (same path as the self avatar) and install them on the matching contact, invalidating its scaled cache so the next render rebuilds + shows it. A `None` result (no avatar / fetch failed) just leaves the placeholder.
    /// Drain attachment blobs a worker verified + stored off-thread: send the attach_have confirm (needs the keypair + checker, which is why it can't run in the worker) so the pusher's pill flips to delivered, then clear the compose wrap and repaint.
    pub(super) fn drain_attach_installed(&mut self) {
        while let Ok(r) = self.attach_installed_rx.try_recv() {
            // A manifest landed: seed the progress bar and repaint; nothing is installed yet.
            if let Some((held, total)) = r.manifest {
                self.attach_chunk_progress.insert(r.content_hash, (held, total));
                self.msg_wrap = None;
                self.scene_dirty = true;
                continue;
            }
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
                        let is_row = m.file.as_ref().is_some_and(|f| f.hash == r.content_hash);
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
                        relay_to: vec![r.sender_pubkey.key], // always the one-device relay copy — responses die on one-directional reverse paths,
                        tag: None,
                    });
                }
            }
            self.msg_wrap = None;
            self.scene_dirty = true;
        }
    }
}

#[cfg(test)]
mod land_tests {
    /// The no-overwrite property that makes a bridge drop safe: a colliding name lands beside the original as "name (2)", never on top of it — a drop can only ADD a file to a remote machine.
    /// The name given is the name landed, and a second drop under it REPLACES the first — no `(2)`, no backup, no third path. The suffixing this replaced hid a bridge-dropped script under a name nobody asked for.
    #[test]
    fn landing_overwrites_under_the_given_name() {
        crate::storage::isolate_test_storage();
        let seed = [0x4Du8; 32];
        let _v = crate::storage::open_session_vault(seed, [0x4E; 32], [0x4F; 32]).expect("vault");
        let dir = std::env::temp_dir().join(format!("photon-land-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let hash = *blake3::hash(b"pigeon body one").as_bytes();
        crate::storage::blob_store(&seed, &hash, b"pigeon body one").expect("store");
        let first = crate::storage::land_blob(&seed, &hash, &dir, "notes.txt").expect("first lands");
        assert!(first.ends_with("notes.txt"), "first keeps the name: {first}");
        // A DIFFERENT blob under the SAME name replaces the first, at the same path.
        let hash2 = *blake3::hash(b"pigeon body two").as_bytes();
        crate::storage::blob_store(&seed, &hash2, b"pigeon body two").expect("store2");
        let second = crate::storage::land_blob(&seed, &hash2, &dir, "notes.txt").expect("second lands");
        assert_eq!(second, first, "the same name lands at the same path");
        assert_eq!(std::fs::read(&second).unwrap(), b"pigeon body two", "and holds the new bytes");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no second file was minted");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
