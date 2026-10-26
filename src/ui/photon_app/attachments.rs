//! Attachments — send/fetch/save of attachment blobs, wire keys, and the install drain.

use super::*;

/// The send cap (typed attachments Phase 1): the picker still hands the bytes over whole, so RAM at pick time is the bound — 256 MB, up from the 25 MB one-frame limit the chunked wire no longer needs.
pub(super) const MAX_ATTACH: usize = 256 * 1024 * 1024;

impl FetchAsk {
    /// Asks in a row that brought none of the blob, counting the last one once it has gone quiet: an ask that brought some of it starts the count again, and an armed fetch never asked counts nothing.
    fn fruitless_after(&self) -> u8 {
        match (self.asked, self.progressed) {
            (None, _) => self.fruitless,
            (Some(_), true) => 0,
            // WHY/PROOF: only the tick checks the count against MAX_TRIES, and re-asks from other paths (a user tap, a reconnect) can each add one before it does — a wrap would hand a hopeless fetch a fresh run of asks.
            (Some(_), false) => self.fruitless.saturating_add(1),
        }
    }
}

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
    pub(super) fn send_attachment_from_path(&mut self, conv: crate::types::ConversationId, path: &str) {
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
        self.send_attachment_from_bytes(conv, name, bytes);
    }

    /// Byte entry (Android picker + desktop drop converge here). Every file — images included — sends BYTE-EXACT: no re-encode exists in this codebase (house doctrine; the JPEG resample overlay was excised 2026-08-20, the attachments rework is parked).
    /// Any conversation at any size (docs/conversations.md): the prepared send re-resolves the conversation, never a contact index.
    pub(super) fn send_attachment_from_bytes(&mut self, conv: crate::types::ConversationId, name: String, bytes: Vec<u8>) {
        if bytes.is_empty() || bytes.len() > MAX_ATTACH {
            self.ready_toast = Some(tr(Msg::AttachmentLimit).into_owned());
            self.ready_toast_screen = None;
            crate::logf!("attach: rejected ({} bytes)", bytes.len());
            return;
        }
        // PREPARE OFF-THREAD (typed attachments 2026-09-10): the kind sniff is cheap, but an image's decode for the previews is hundreds of ms on a phone photo and the AV1 encode more — the worker hands back the typed extras and the drain sends the row (re-resolving the contact by handle, the index may have moved). A RAW gets a temp copy for limbus (file-only reader), minted in the worker and removed by the drain.
        let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        let tx = self.attach_prepared_tx.clone();
        queue_job(&self.attach_job_tx, move || {
            let kind = crate::types::sniff(&bytes, &name);
            let hash = *blake3::hash(&bytes).as_bytes();
            let raw_tmp = raw_temp_path(kind, &hash, &bytes);
            let p = crate::ui::attach_preview::prepare(&bytes, &name, raw_tmp.as_deref());
            // Sealed to disk HERE, not in the drain: the row and the push need only the manifest.
            // WRAPPED (storage/wrapped.rs, 2026-10-17): the original rests and travels inside its own VSF document, named by the document's header hb; the raw hash keeps naming it in the row's identity and for older builds (an alias), until v134. A wrap that fails stores the raw form as before.
            let (wrap, manifest) = match crate::storage::blob_store_wrapped(&seed, &bytes, None) {
                Ok((hb, m)) => (Some(hb), m),
                Err(e) => {
                    crate::logf!("attach: wrap failed ({}) — storing the raw original", e);
                    match crate::storage::blob_store_any(&seed, &hash, &bytes) {
                        Ok(m) => (None, m),
                        Err(e) => {
                            crate::logf!("attach: blob store failed: {}", e);
                            if let Some(t) = raw_tmp.as_ref() {
                                let _ = std::fs::remove_file(t);
                            }
                            return;
                        }
                    }
                }
            };
            // THE HEAD (flag day 2026-10-07): name, kind, dims, the micro thumb and the preview, wrapped as one small VSF blob and sealed here too — the row on the chain is the original's hash and this head's hash, nothing else.
            let head = crate::types::AttachHead { name: name.clone(), kind: p.meta.kind, dims: p.meta.dims, micro: p.preview.clone(), preview: p.blob.clone() };
            let head_hash = head.encode().and_then(|hb| {
                let hh = *blake3::hash(&hb).as_bytes();
                match crate::storage::blob_store(&seed, &hh, &hb) {
                    Ok(()) => Some(hh),
                    Err(e) => {
                        crate::logf!("attach: head store failed: {} — sending without a head", e);
                        None
                    }
                }
            });
            let _ = tx.send(super::AttachPrepared { conv, name, bytes, meta: p.meta, preview: p.preview, blob: p.blob, raw_tmp, hash, manifest, head: head_hash, wrap });
        });
    }

    /// Drain prepared picks: stage the typed extras for the row and send (attach_send_now).
    pub(super) fn drain_attach_prepared(&mut self) {
        while let Ok(p) = self.attach_prepared_rx.try_recv() {
            if self.conv_by_id(p.conv).is_none() {
                crate::log("attach: prepared pick's conversation is gone — dropped");
                continue;
            }
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
            // A headed send needs no separate preview blob: the preview's bytes live inside the head. Only a head that failed to store falls back to the pre-flag-day shape (preview blob + inline extras).
            let mut meta = p.meta;
            if p.head.is_none() {
                if let (Some(blob), Some(ph), Some(seed)) = (p.blob.as_ref(), meta.preview_hash, self.session.as_ref().map(|s| s.identity_seed)) {
                    if let Err(e) = crate::storage::blob_store(&seed, &ph, blob) {
                        crate::logf!("attach: preview blob store failed: {}", e);
                        meta.preview_hash = None;
                    }
                }
            }
            self.attach_send_now(p.conv, p.name, p.bytes, meta, p.preview, p.hash, p.manifest, p.head, p.wrap);
        }
    }

    /// The actual send: cap 25MB, blob sealed to disk, the row = an ATTACHMENT_PREFIX content string riding the ordinary chain send (bubble, ACK, fleet sync, tombstones all inherited), then the blob itself pushed over PT.
    pub(super) fn attach_send_now(&mut self, conv: crate::types::ConversationId, name: String, bytes: Vec<u8>, meta: crate::types::AttachMeta, preview: Vec<u8>, hash: [u8; 32], manifest: Option<crate::storage::BlobManifest>, head: Option<[u8; 32]>, wrap: Option<[u8; 32]>) {
        // Images and music travel NAMELESS (Nick: "the user typed nothing. just an image" / "no name, just the waveform") — a filename is metadata nobody chose to send; the receiver derives extensions from the bytes' own magic.
        let wire_name = if meta.kind.is_image() || meta.kind == crate::types::AttachKind::Audio { "" } else { name.as_str() };
        // HEADED (flag day 2026-10-07): the chain row is two pointers; THIS device's row is hydrated at once from what it just built (runtime fields the writers never carry for a headed row), so the sender sees its picture and name without reading the head back.
        let (file, meta) = match head {
            Some(hh) => (
                crate::types::AttachRef::pointer(hash, bytes.len() as u64, crate::types::AttachRole::File, hh),
                crate::types::AttachMeta { preview_hash: meta.preview_hash.map(|_| hh), ..meta },
            ),
            None => (crate::types::AttachRef::file(hash, wire_name, bytes.len() as u64), meta),
        };
        // The wrapper's hb rides the row as `fw` (metadata, never identity — row_control.rs).
        let file = crate::types::AttachRef { wrap, ..file };
        // The row: ordinary chain send (or fleet-forward on a chainless device) — everything downstream treats it as a normal message. Its typed extras are STAGED so the minted row carries them before the transmit reads it.
        self.attach_stage = Some((meta, preview, file));
        if !self.send_in_conversation(conv, "", None) {
            self.attach_stage = None;
            crate::log("attach: row send failed (no chain, no fleet) — attachment stays local");
        }
        if head.is_some() {
            if let Some(view) = self.conv_by_id_mut(conv) {
                if let Some(m) = view.messages.iter_mut().rev().find(|m| m.is_outgoing && m.file.as_ref().is_some_and(|f| f.hash == hash)) {
                    m.head_name = name.clone();
                }
            }
        }
        // WHO GETS THE PUSH: a pair's friend under the pair's own token; in a roster conversation every friend standing in it, under the conversation's token (its history key seals the blob, so any member opens it). A member we never friended has no address here and fetches by attach_req.
        let targets: Vec<(usize, Option<[u8; 32]>)> = match self.roster_id_of(conv) {
            Some(gid) => {
                let token = self.conversation_token_of(conv);
                let standing = self.molecule_rosters.iter().find(|(g, _)| *g == gid).map(|(_, r)| r.standing()).unwrap_or_default();
                (0..self.contacts.len()).filter(|&ci| !self.contacts[ci].is_sibling && standing.contains(&self.contacts[ci].handle_hash)).map(|ci| (ci, token)).collect()
            }
            None => self.pair_contact_of(conv).map(|ci| (ci, None)).into_iter().collect(),
        };
        // The original goes out as the WRAPPER under its hb when it was stored wrapped (`manifest` is then the wrapper's chunks); a wrapper-aware friend verifies and keeps it as is, an older build refuses the unfamiliar name and fetches by `fh`, which the serve path answers unwrapped.
        let original = wrap.unwrap_or(hash);
        for (ci, token) in targets {
            // The head (or, unheaded, the preview blob) first — small, the name and the picture the friend sees before the file lands — then the original: one frame for a small file, manifest + chunks for a large one.
            match head {
                Some(hh) => self.send_attach_blob(ci, &hh, token),
                None => {
                    if let Some(ph) = meta.preview_hash {
                        self.send_attach_blob(ci, &ph, token);
                    }
                }
            }
            match manifest.clone() {
                Some(m) => self.send_attach_chunks(ci, &original, m, token),
                None => self.send_attach_blob(ci, &original, token),
            }
        }
        self.msg_wrap = None;
        self.scene_dirty = true;
        crate::logf!(
            "attach: sent {} ({} bytes)",
            crate::deglyph_for_log(&name),
            bytes.len()
        );
    }

    /// The name the rows and progress maps know a blob by: a WRAPPER's hb maps to the raw hash of the row that names it (`fw`); anything else is its own name.
    pub(super) fn raw_name_of(&self, h: &[u8; 32]) -> [u8; 32] {
        self.conversations
            .iter()
            .flat_map(|c| c.messages.iter())
            .find_map(|m| m.file.as_ref().filter(|f| f.wrap == Some(*h)).map(|f| f.hash))
            .unwrap_or(*h)
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
    /// The load, the seal and the dispatch run on the attach worker: the UI thread makes no vault call (2026-09-18 ANR).
    /// `token` names the conversation the blob belongs to when it is not the pair's own (a roster conversation's push to one of its members).
    pub(super) fn send_attach_blob(&mut self, ci: usize, content_hash: &[u8; 32], token: Option<[u8; 32]>) {
        let token_for = token;
        let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        let bar = self.raw_name_of(content_hash);
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
            let Some(token) = token_for.or_else(|| {
                self.friendship_chains
                    .iter()
                    .find(|(id, _)| Some(*id) == c.friendship_id)
                    .map(|(_, ch)| ch.conversation_token)
            }) else {
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
        let (Some(kp), Some(checker)) =
            (self.device_keypair.as_ref(), self.status_checker.as_ref())
        else {
            return;
        };
        let (kp_pub, kp_sec) = (*kp.public.as_bytes(), *kp.secret.as_bytes());
        let dispatch = checker.history_dispatch();
        let content_hash = *content_hash;
        queue_job(&self.attach_job_tx, move || {
            let Some(plain) = crate::storage::blob_load(&seed, &content_hash) else {
                crate::log("attach: blob missing locally — nothing to push");
                return;
            };
            let Ok(sealed) = kete::encrypt_bytes(&plain, &wire_key) else {
                return;
            };
            match crate::network::fgtw::protocol::build_attach_blob_vsf(&token, &content_hash, sealed, &kp_pub, &kp_sec) {
                Ok(vsf_bytes) => {
                    let _ = dispatch.send(crate::network::status::HistorySendRequest {
                        peer_addr,
                        alt_addr,
                        recipient_pubkey: device,
                        vsf_bytes,
                        relay_to,
                        tag: Some(bar), // the bar for THIS blob reads its transfer by this tag (the row's raw hash, also for a wrapper)
                    });
                    crate::log("attach: blob dispatched over PT");
                }
                Err(e) => crate::logf!("attach: blob frame build failed: {}", e),
            }
        });
    }

    /// Push a CHUNKED blob to the friend: the sealed manifest, then every chunk, each its own PT frame — sealed and dispatched from the worker (256 seals of 256 KB do not belong on the render thread).
    pub(super) fn send_attach_chunks(&mut self, ci: usize, content_hash: &[u8; 32], m: crate::storage::BlobManifest, token: Option<[u8; 32]>) {
        let token_for = token;
        let (device, addr_pair, relay_to, token) = {
            let Some(c) = self.contacts.get(ci) else {
                return;
            };
            if self.our_party_id(c).is_none_or(|us| c.remote_count(&us) == 0) || c.is_sibling {
                return;
            }
            let Some(token) = token_for.or_else(|| {
                self.friendship_chains
                    .iter()
                    .find(|(id, _)| Some(*id) == c.friendship_id)
                    .map(|(_, ch)| ch.conversation_token)
            }) else {
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
        // The bar speaks the row's RAW hash even when the chunks are a wrapper's.
        let bar = self.raw_name_of(&content_hash);
        self.attach_send_total.insert(bar, m.chunks.len() as u32);
        queue_job(&self.attach_job_tx, move || {
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
                        send(v, Some(bar));
                        sent += 1;
                    }
                    Err(e) => crate::logf!("attach: chunk {} frame build failed: {}", i, e),
                }
            }
            crate::logf!("attach: manifest + {} of {} chunk(s) dispatched over PT", sent, m.chunks.len());
        });
    }

    /// Ask for a missing blob: attach_req to the conversation's friend device AND every online sibling — whoever holds it answers with an attach_blob (or, for a chunked blob, the manifest + chunks; a held manifest turns the ask into a RESUME carrying the want bitmap of the chunks still missing).
    /// Fetch a blob a row in ANY conversation names — the conversation-keyed door the row actions use (docs/conversations.md). A roster-backed conversation asks its members' devices and ours the way a pair asks its friend's.
    pub(super) fn attach_fetch_in(&mut self, id: crate::types::ConversationId, content_hash: &[u8; 32]) {
        match self.pair_contact_of(id) {
            Some(ci) => self.attach_fetch(ci, content_hash),
            None => self.attach_fetch_conversation(id, content_hash),
        }
    }

    pub(super) fn attach_fetch(&mut self, sci: usize, content_hash: &[u8; 32]) {
        if let Some(id) = self.contacts.get(sci).and_then(|c| self.our_party_id(c).map(|us| c.conversation(&us).id())) {
            self.attach_fetch_conversation(id, content_hash);
        }
    }

    /// Fetch a blob a row of conversation `id` names, from the conversation's other members' devices and our own siblings — one device per ask, ranked, rotating on each re-ask. Any conversation at any size (docs/conversations.md).
    pub(super) fn attach_fetch_conversation(&mut self, id: crate::types::ConversationId, content_hash: &[u8; 32]) {
        // Automatic paths gate on recording_wanted before calling, so reaching here is a deliberate fetch.
        self.space_unevict(content_hash);
        // A STREAMING DEVICE IS NEVER ABANDONED (Emma 2026-10-21: 432 re-asks in eight minutes, two devices streaming the same blobs): an ask whose device is not quiet and has answered since stands, so a tap or a second caller waits on it instead of asking another device to stream the same blob alongside.
        // An ask with no answer yet does not block a deliberate fetch: with no timer to end a silence, a tap is the edge that moves it on.
        let streaming = |f: &&FetchAsk| f.asked.is_some_and(|d| self.attach_heard.get(&d).is_some_and(|h| *h > f.at));
        if let Some(dev) = self.attach_fetch_inflight.get(content_hash).filter(|f| !self.fetch_quiet(f)).filter(streaming).and_then(|f| f.asked) {
            crate::logf!("attach: {}… already asked of device {}, which is not quiet — waiting on it", hex::encode(&content_hash[..4]), crate::fp(&dev));
            return;
        }
        // Token: the friendship token when one exists; else (self-conversation — no chains) the handle hash. Sibling exchanges seal under the FLEET key regardless of token, so the fallback only ever reaches sibling responders, where it's a plain discriminator.
        // The conversation's own wire token (stored with its keys; your own conversation's is the [us, us] derivation). Sibling exchanges seal under the FLEET key regardless of token, so for a conversation with nobody else in it the token only ever reaches sibling responders, where it is a plain discriminator.
        let Some(token) = self.conversation_token_of(id) else {
            return;
        };
        // The other members: a friend's devices are asked as in a pair; a member we never friended is asked too, by the roster (below).
        let members: Vec<[u8; 32]> = self.conv_by_id(id).map(|v| v.participants().to_vec()).unwrap_or_default();
        let (Some(kp), Some(checker)) =
            (self.device_keypair.as_ref(), self.status_checker.as_ref())
        else {
            return;
        };
        // THE ASK (storage/wrapped.rs, until v134): a row naming a WRAPPER (`fw`) asks for it by its hb; a holder still on an older build knows only the raw hash, so the rounds alternate — whichever form answers is stored, and every load by the row's raw hash reads either. The in-flight bookkeeping stays keyed by the raw hash.
        let wrap = self.conversations.iter().flat_map(|c| c.messages.iter()).find_map(|m| m.file.as_ref().filter(|f| f.hash == *content_hash).and_then(|f| f.wrap));
        let round = self.attach_fetch_inflight.get(content_hash).map_or(0, |f| f.tries);
        let ask = match wrap {
            Some(hb) if round % 2 == 0 => hb,
            _ => *content_hash,
        };
        // ONE DEVICE PER ASK (field 2026-09-17, Nick: "re-uploads/replications of pigeons"): the request used to fan out to the friend's every device AND all our siblings at once, and every holder answered with the whole blob — a 142 MB pigeon served in full by three devices, ~300 KB pigeons landing nine times each. Now the candidates are ranked — the friend's devices with a proven direct path first, then the friend's other online devices, then our online siblings, then the rest — one is asked, and each re-ask (attach_fetch_retry_tick, once the asked device goes quiet) moves to the next.
        let tries = round;
        let mut targets: Vec<(
            u8,
            std::net::SocketAddr,
            Option<std::net::SocketAddr>,
            [u8; 32],
            Vec<[u8; 32]>,
        )> = Vec::new();
        // A PULL (holdings.rs, removal safety): the leaving device holds the only copy and asked us to take it — it outranks everyone; every other rank sits one lower.
        let prefer = self.attach_fetch_prefer.get(content_hash).copied();
        let ranked = |k: [u8; 32], rank: u8| if prefer == Some(k) { 0 } else { rank + 1 };
        for c in &self.contacts {
            let is_friend = !c.is_sibling && members.contains(&c.handle_hash);
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
                    targets.push((ranked(k, rank), a, alt, k, relay));
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
                    targets.push((ranked(ep.pubkey, if ep.online { 1 } else { 3 }), addr, alt, ep.pubkey, vec![ep.pubkey]));
                }
            }
        }
        // STRANGERS IN THE ROOM (Nick 2026-10-26: "we know who's in it"): a standing member we never friended is asked like a friend — their devices from the roster's fold (member_devices), their addresses from the phonebook (device_addresses), the relay alone when none is known. They rank with a friend's offline devices: after the friends' live paths and our own siblings, before nobody.
        let us = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed));
        let strangers: Vec<[u8; 32]> = members.iter().copied().filter(|p| Some(*p) != us && !self.contacts.iter().any(|c| !c.is_sibling && c.handle_hash == *p)).collect();
        for party in &strangers {
            for dev in super::molecules::member_devices(&self.contacts, &self.molecule_peers, party) {
                let addrs = self.device_addresses(&dev);
                let (addr, alt) = (addrs.first().copied().unwrap_or(crate::network::status::RELAY_ADDR), addrs.get(1).copied());
                targets.push((ranked(dev, 3), addr, alt, dev, vec![dev]));
            }
        }
        targets.sort_by_key(|t| t.0);
        // A parked sibling cannot answer: asking it burns one of MAX_TRIES rounds and a relay leg into a closed pipe (Emma 2026-10-21: 72 asks at two old phones that never came online).
        // It stays a candidate only when nobody else is left, so a fetch is never dropped for want of a live device.
        let parked: Vec<[u8; 32]> = self.contacts.iter().filter(|c| c.sibling_parked()).filter_map(|c| c.device_key()).collect();
        if targets.iter().any(|t| !parked.contains(&t.3)) {
            targets.retain(|t| !parked.contains(&t.3));
        }
        if targets.is_empty() {
            crate::log("attach: fetch has nobody to ask — no device of the friend or ours has an address");
            return;
        }
        let (rank, peer_addr, alt_addr, recipient_pubkey, relay_to) = targets[tries as usize % targets.len()].clone();
        let (kp_pub, kp_sec) = (*kp.public.as_bytes(), *kp.secret.as_bytes());
        let dispatch = checker.history_dispatch();
        let candidate = format!("rank {}, candidate {} of {}", rank, tries as usize % targets.len() + 1, targets.len());
        // OFF THE UI THREAD: the resume's want bitmap is a manifest read plus one read per chunk, so the worker builds the ask and dispatches it; on the attach worker it also sees every chunk already queued there for storing.
        queue_job(&self.attach_job_tx, move || {
            // Resume: the manifest is here and some chunks are not → ask only for those.
            let want: Option<Vec<u8>> = crate::storage::blob_chunks_held(&ask)
                .filter(|held| held.iter().any(|h| !h))
                .map(|held| crate::storage::BlobManifest::want_bitmap(&held));
            if let Some(w) = want.as_ref() {
                let missing = w.iter().map(|b| b.count_ones()).sum::<u32>();
                crate::logf!("attach: resuming — {} chunk(s) still wanted", missing);
            }
            let Ok(vsf_bytes) = crate::network::fgtw::protocol::build_attach_req_want_vsf(&token, &ask, want.as_deref(), &kp_pub, &kp_sec) else {
                return;
            };
            let _ = dispatch.send(crate::network::status::HistorySendRequest {
                peer_addr,
                alt_addr,
                recipient_pubkey,
                vsf_bytes,
                relay_to,
                tag: None,
            });
            crate::logf!("attach: fetch request dispatched to device {} ({})", crate::fp(&recipient_pubkey), candidate);
        });
        let conv_id = id;
        let fruitless = self.attach_fetch_inflight.get(content_hash).map_or(0, |f| f.fruitless_after());
        let asked_online = self.device_online(&recipient_pubkey) == Some(true);
        self.attach_fetch_inflight.insert(
            *content_hash,
            FetchAsk {
                conv: conv_id,
                asked: Some(recipient_pubkey),
                at: std::time::Instant::now(),
                asked_online,
                tries: tries.saturating_add(1), // WHY/PROOF: a u8 of fetch rounds for a blob no device answers — it outlives 255, and saturating keeps the candidate rotation from wrapping back to rank 0's first try
                fruitless,
                progressed: false,
                ended: false,
            },
        );
    }

    /// The device's online verdict as this device holds it — a contact's own device, or one of a friend's other known devices; None when no contact names it.
    fn device_online(&self, dev: &[u8; 32]) -> Option<bool> {
        self.contacts.iter().find_map(|c| {
            if c.device_key() == Some(*dev) {
                return Some(c.is_online);
            }
            c.device_endpoints.iter().find(|ep| ep.pubkey == *dev).map(|ep| ep.online)
        })
    }

    /// QUIET, from edges alone (Nick 2026-10-21: "I don't like timers"): every ask ends in an event — the blob's frames (progress), "not held" (attach_none), the transport giving up on the device (a dead address, the relay's offline word), or the device's offline verdict.
    /// An armed fetch never asked is due at once: a friend's live attachment row records its pushing device as already asked (fetch_expect_push), so no push window is needed.
    /// Progress is per DEVICE, not per blob: a holder serves our asks one after another, so an ask queued behind other blobs shows nothing of its own until its turn, and the device streaming them is not quiet.
    fn fetch_quiet(&self, f: &FetchAsk) -> bool {
        let Some(dev) = f.asked else {
            return true;
        };
        if f.ended {
            return true;
        }
        // The offline verdict ends an ask made while the device was up, or one it had started answering.
        let heard = self.attach_heard.get(&dev).is_some_and(|h| *h > f.at);
        self.device_online(&dev) == Some(false) && (f.asked_online || heard)
    }

    /// The asked device does not hold the blob: that ask is over (one fruitless ask), and the tick asks the next device.
    pub(super) fn fetch_answered_none(&mut self, hash: &[u8; 32], from: &[u8; 32]) {
        if let Some(f) = self.attach_fetch_inflight.get_mut(hash).filter(|f| f.asked == Some(*from)) {
            f.ended = true;
            crate::logf!("attach: device {} holds none of {}… — asking the next", crate::fp(from), hex::encode(&hash[..4]));
        }
    }

    /// The transport gave up on a device (by key, or by an address one of its endpoints carries): every ask aimed there is over.
    pub(super) fn fetch_peer_unreachable(&mut self, addr: Option<std::net::SocketAddr>, device: Option<[u8; 32]>) {
        let same = |a: std::net::SocketAddr, b: std::net::SocketAddr| a.ip().to_canonical() == b.ip().to_canonical() && a.port() == b.port();
        let devs: crate::linear_map::SortedSet<[u8; 32]> = self
            .attach_fetch_inflight
            .values()
            .filter_map(|f| f.asked)
            .filter(|d| {
                device == Some(*d)
                    || addr.is_some_and(|a| {
                        self.contacts.iter().any(|c| {
                            (c.device_key() == Some(*d) && (c.ip.is_some_and(|i| same(i, a)) || c.validated_path.is_some_and(|(p, _)| same(p, a))))
                                || c.device_endpoints.iter().any(|ep| ep.pubkey == *d && (ep.lan.is_some_and(|l| same(l, a)) || ep.public.is_some_and(|p| same(p, a))))
                        })
                    })
            })
            .collect();
        for f in self.attach_fetch_inflight.values_mut().filter(|f| f.asked.is_some_and(|d| devs.contains(&d)) && !f.ended) {
            f.ended = true;
        }
        for d in &devs {
            crate::logf!("attach: device {} is unreachable — its asks move on", crate::fp(d));
        }
    }

    /// A friend's live attachment row: the device that sent it pushes the blob right behind it, so that device counts as asked — its frames are the answer, and replication does not ask anyone else meanwhile.
    pub(super) fn fetch_expect_push(&mut self, hash: [u8; 32], conv: crate::types::ConversationId, from: [u8; 32]) {
        if self.attach_fetch_inflight.contains_key(&hash) || crate::storage::blob_present_known(&hash) == Some(true) {
            return;
        }
        let asked_online = self.device_online(&from) != Some(false);
        self.attach_fetch_inflight.insert(hash, FetchAsk { conv, asked: Some(from), at: std::time::Instant::now(), asked_online, tries: 1, fruitless: 0, progressed: false, ended: false });
    }

    /// THE FLEET HOLDS EVERYTHING (Nick 2026-10-07: "if I have five devices, all five should get copies of waves, pigeons, beams, photons, all of it… be sane"). One pass per edge (`replicate_dirty`: rows arrived, presence answers landed, the network went unmetered): every live file row in every conversation whose bytes this device is KNOWN not to hold — pigeons, wave recordings, wave envelopes, previews' originals — is armed ONCE per session in the fetch map, and the retry tick asks one device at a time (friend's devices and our siblings, easiest first) until it lands. An armed fetch is asked on the next tick; a friend's live attachment row is already recorded as asked of the device pushing it (fetch_expect_push), so nobody else is asked while that push runs. A row whose presence is not known yet queues its probe here and is picked up on the edge its answer raises. Sane: a metered network arms nothing (Android reports it; desktops are never metered here).
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
        if !std::mem::take(&mut self.replicate_dirty) || self.session.is_none() {
            return;
        }
        let now = std::time::Instant::now();
        let mut armed = 0usize;
        // Every conversation at any size (docs/conversations.md) — a sibling's is the bridge terminal and holds no files.
        let sibling_convs: Vec<crate::types::ConversationId> = self.contacts.iter().filter(|c| c.is_sibling).filter_map(|c| self.our_party_id(c).map(|us| c.conversation(&us).id())).collect();
        let conv_ids: Vec<crate::types::ConversationId> = self.conversations.iter().map(|v| v.id()).filter(|id| !sibling_convs.contains(id)).collect();
        for cid in conv_ids {
            // Each row's ORIGINAL (unmetered only) and its HEAD (any network — a few KB, the name and the picture). A device that opted out of holding waves (`waves.hold` off — a watch) skips recordings.
            let hashes: Vec<[u8; 32]> = self
                .conv_by_id(cid)
                .map(|v| {
                    v.messages
                        .iter()
                        .filter(|m| !m.deleted)
                        .flat_map(|m| {
                            // A recording also waits while the disk is below its floor, and never returns once evicted here (space.rs).
                            let original = m.file.as_ref().filter(|f| !metered && (!m.is_wave_recording() || self.recording_wanted(&f.hash))).map(|f| f.hash);
                            let head = m.file.as_ref().and_then(|f| f.head);
                            original.into_iter().chain(head)
                        })
                        .collect()
                })
                .unwrap_or_default();
            for h in hashes {
                if self.attach_auto_fetched.contains(&h) || self.attach_fetch_inflight.contains_key(&h) {
                    continue;
                }
                if crate::storage::blob_present_known(&h) == Some(false) {
                    self.attach_auto_fetched.insert(h);
                    self.attach_fetch_inflight.insert(h, FetchAsk { conv: cid, asked: None, at: now, asked_online: false, tries: 0, fruitless: 0, progressed: false, ended: false });
                    armed += 1;
                }
            }
        }
        if armed > 0 {
            crate::logf!("REPLICATE: {} file(s) not on this device — fetching each from the fleet, one device at a time", armed);
        }
    }

    /// Move each fetch whose asked device went QUIET ([`Self::fetch_quiet`]) to the next candidate, and let go after MAX_TRIES asks in a row that brought none of the blob. A device still sending is never left for another, and a blob that lands leaves the map on its own edge (drain_attach_installed).
    pub(super) fn attach_fetch_retry_tick(&mut self) {
        const MAX_TRIES: u8 = 8;
        for (hash, conv, from) in std::mem::take(&mut self.attach_expect_pushes) {
            self.fetch_expect_push(hash, conv, from);
        }
        let due: Vec<[u8; 32]> = self.attach_fetch_inflight.iter().filter(|(_, f)| self.fetch_quiet(f)).map(|(h, _)| *h).collect();
        for hash in due {
            // Presence from the cache alone (the UI thread makes no vault call): held → done; not known yet → its probe is queued, and the tick its answer raises looks again.
            match crate::storage::blob_present_known(&hash) {
                Some(true) => {
                    self.attach_fetch_inflight.remove(&hash);
                    self.holdings_pull_settled(&hash, true);
                    continue;
                }
                None => continue,
                Some(false) => {}
            }
            let Some(f) = self.attach_fetch_inflight.get(&hash) else { continue };
            let (id, tries, fruitless, asked) = (f.conv, f.tries, f.fruitless_after(), f.asked);
            // The conversation may be gone since the ask — its fetch is let go, never re-aimed elsewhere.
            if fruitless >= MAX_TRIES || self.conv_by_id(id).is_none() {
                crate::logf!("attach: fetch of {}… let go after {} asks — {} in a row brought none of it, or its conversation was removed", hex::encode(&hash[..4]), tries, fruitless);
                self.attach_fetch_inflight.remove(&hash);
                self.holdings_pull_settled(&hash, false);
                continue;
            }
            match asked {
                None => crate::logf!("attach: replicating {}… — first ask", hex::encode(&hash[..4])),
                Some(dev) => crate::logf!("attach: fetch of {}… — device {} went quiet, asking the next ({} of {} fruitless)", hex::encode(&hash[..4]), crate::fp(&dev), fruitless, MAX_TRIES),
            }
            self.attach_fetch_conversation(id, &hash);
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
    pub(super) fn attach_save(&mut self, name: &str, content_hash: &[u8; 32]) -> Result<String, SaveMiss> {
        let seed = self.session.as_ref().map(|s| s.identity_seed).ok_or(SaveMiss::Failed)?;
        // Android: stage in the app's private dir, then hand the file to the PUBLIC Downloads/Photon (MediaStore) — the private dir was invisible to the user and every other app (Nick 2026-09-28: "somewhere sane and shared").
        #[cfg(target_os = "android")]
        let base = crate::storage::photon_config_dir().map_err(|_| SaveMiss::Failed)?.join("staging");
        #[cfg(not(target_os = "android"))]
        let base = dirs::download_dir().ok_or(SaveMiss::Failed)?;
        let _ = std::fs::create_dir_all(&base);
        let name = Self::attach_file_name(&seed, name, content_hash);
        let name = name.as_str();
        // macOS: the system save panel, opening in Downloads with the name filled in — a place the user picked is writable with no privacy prompt (Nick 2026-10-26, after "unable to save pigeons on mac"); cancelling is no failure.
        #[cfg(target_os = "macos")]
        let (base, name) = {
            let dest = fluor::host::file_drag::save_dialog(name, Some(&base)).ok_or(SaveMiss::Cancelled)?;
            let file = dest.file_name().map(|f| f.to_string_lossy().into_owned()).ok_or(SaveMiss::Failed)?;
            (dest.parent().map(|p| p.to_path_buf()).ok_or(SaveMiss::Failed)?, file)
        };
        #[cfg(target_os = "macos")]
        let name = name.as_str();
        // Stream the blob under exactly that name, replacing any earlier save — the same landing a bridge pigeon does, factored out.
        let landed = crate::storage::land_blob(&seed, content_hash, &base, name).ok_or(SaveMiss::Failed)?;
        #[cfg(target_os = "android")]
        {
            let staged = std::path::PathBuf::from(&landed);
            let shared = crate::platform::jni_android::save_to_downloads(&staged, name);
            let _ = std::fs::remove_file(&staged); // the staging copy is done with either way
            return shared.ok_or(SaveMiss::Failed);
        }
        #[cfg(not(target_os = "android"))]
        Ok(landed)
    }

    /// The name a saved or dragged file lands under: its own, or for a nameless attachment (images travel without filenames) its hash prefix with a magic-sniffed extension.
    pub(super) fn attach_file_name(seed: &[u8; 32], name: &str, content_hash: &[u8; 32]) -> String {
        if !name.is_empty() {
            return name.to_string();
        }
        let head = crate::storage::blob_load(seed, content_hash).unwrap_or_default();
        format!("photon-{}.{}", hex::encode(&content_hash[..8]), crate::types::sniff_ext(&head))
    }

    /// Drain completed peer-avatar downloads: colour-convert the VSF-RGB pixels to the display buffer (same path as the self avatar) and install them on the matching contact, invalidating its scaled cache so the next render rebuilds + shows it. A `None` result (no avatar / fetch failed) just leaves the placeholder.
    /// Drain attachment blobs a worker verified + stored off-thread: send the attach_have confirm (needs the keypair + checker, which is why it can't run in the worker) so the pusher's pill flips to delivered, then clear the compose wrap and repaint.
    pub(super) fn drain_attach_installed(&mut self) {
        while let Ok(mut r) = self.attach_installed_rx.try_recv() {
            // A WRAPPER in flight (storage/wrapped.rs) is named by its hb on the wire; every map here — progress, sniff verdicts, attach_have — speaks the row's RAW hash.
            if let Some(raw) = self.conversations.iter().flat_map(|c| c.messages.iter()).find_map(|m| m.file.as_ref().filter(|f| f.wrap == Some(r.content_hash)).map(|f| f.hash)) {
                r.content_hash = raw;
            }
            // A piece of a blob we asked for landed: the ask brought something, so it does not count toward letting go (attach_fetch_retry_tick).
            if let Some(f) = self.attach_fetch_inflight.get_mut(&r.content_hash) {
                f.progressed = true;
            }
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
                self.attach_sniffed.insert(r.content_hash, local);
                for conv in self.conversations.iter_mut() {
                    for m in conv.messages.iter_mut() {
                        let is_row = m.file.as_ref().is_some_and(|f| f.hash == r.content_hash);
                        // A headed row whose head is not read yet is left alone — a bare kind here would read as hydrated and its head would never load; the verdict applies when the head hydrates it (drain_heads_loaded).
                        if !is_row || m.head_pending().is_some() {
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
            // The landing is the fetch's end edge: nothing re-asks for a blob that is here.
            self.attach_fetch_inflight.remove(&r.content_hash);
            // A blob a leaver asked us to take (holdings.rs): one fewer item at risk on its screen.
            self.holdings_pull_settled(&r.content_hash, true);
            // A recording landing is the disk's edge (space.rs).
            if self.conversations.iter().flat_map(|c| c.messages.iter()).any(|m| m.is_wave_recording() && m.file.as_ref().is_some_and(|f| f.hash == r.content_hash)) {
                self.space_check("a recording landed");
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

#[cfg(test)]
mod fetch_tests {
    use super::super::FetchAsk;

    fn ask(asked: bool, fruitless: u8, progressed: bool) -> FetchAsk {
        FetchAsk { conv: crate::types::FriendshipId::from_bytes([7; 32]), asked: asked.then_some([9; 32]), at: std::time::Instant::now(), asked_online: true, tries: 3, fruitless, progressed, ended: false }
    }

    /// The let-go count: an ask that brought none of the blob adds one, an ask that brought any of it starts over, and an armed fetch nobody has asked yet adds nothing.
    #[test]
    fn fruitless_counts_only_asks_that_brought_nothing() {
        assert_eq!(ask(true, 4, false).fruitless_after(), 5, "a quiet ask with nothing landed counts");
        assert_eq!(ask(true, 4, true).fruitless_after(), 0, "a quiet ask that brought chunks starts the count again");
        assert_eq!(ask(false, 0, false).fruitless_after(), 0, "an armed fetch was never asked");
        assert_eq!(ask(true, u8::MAX, false).fruitless_after(), u8::MAX, "the count saturates");
    }
}

/// Why a Save did not land: the user cancelled the save panel (no toast, nothing went wrong), or it failed.
// Only macOS has a save panel to cancel; the other platforms save straight to Downloads.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) enum SaveMiss {
    Cancelled,
    Failed,
}

impl PhotonApp {
    /// Hand the pressed file to the host's drag-out (fluor host/file_drag.rs): its name now, its bytes read from the vault when a drop asks — no file exists anywhere until the drop writes one where the user put it.
    #[cfg(not(target_os = "android"))]
    pub(super) fn start_file_drag(&mut self) -> Option<fluor::host::EventResponse> {
        let (hash, name, _, _) = self.file_drag_from.take()?;
        let seed = self.session.as_ref()?.identity_seed;
        let name = Self::attach_file_name(&seed, &name, &hash);
        // The press is the OS's now: its release never reaches us, so nothing of ours may wait on it.
        self.press_held = false;
        crate::logf!("attach: {}… dragged out of the window as {}", hex::encode(&hash[..4]), name);
        fluor::host::file_drag::request(fluor::host::file_drag::DragFile { name, bytes: std::sync::Arc::new(move || crate::storage::blob_load_named(&hash)) });
        Some(fluor::host::EventResponse::StartFileDrag)
    }
}

impl PhotonApp {
    /// May this device trade files with us in the conversation `token` names? A contact's device, or a device of a member standing in that very conversation — a member we never friended asks, serves and answers like a friend (docs/conversations.md; only members hold the token).
    pub(super) fn attach_peer_known(&self, device: &[u8; 32], token: &[u8; 32]) -> bool {
        self.contacts.iter().any(|c| c.knows_device(device))
            || self.molecule_rosters.iter().find(|(g, _)| g.token() == *token).is_some_and(|(_, r)| self.molecule_peers.iter().any(|p| p.knows_device(device) && r.is_standing(&p.party)))
    }
}
