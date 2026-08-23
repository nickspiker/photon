//! Attachments — send/fetch/save of attachment blobs, wire keys, and the install drain.

use super::*;

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
        self.attach_send_now(ci, name, bytes);
    }

    /// The actual send: cap 25MB, blob sealed to disk, the row = an ATTACHMENT_PREFIX content string riding the ordinary chain send (bubble, ACK, fleet sync, tombstones all inherited), then the blob itself pushed over PT.
    pub(super) fn attach_send_now(&mut self, ci: usize, name: String, bytes: Vec<u8>) {
        const MAX_ATTACH: usize = 25 * 1024 * 1024;
        if bytes.is_empty() || bytes.len() > MAX_ATTACH {
            self.ready_toast =
                Some("attachment limit is 25 MB".to_string());
            self.ready_toast_screen = None;
            crate::logf!("attach: rejected ({} bytes)", bytes.len());
            return;
        }
        let hash = *blake3::hash(&bytes).as_bytes();
        let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        if let Err(e) = crate::storage::blob_store(&seed, &hash, &bytes) {
            crate::logf!("attach: blob store failed: {}", e);
            return;
        }
        let content = crate::types::attachment_content(&hash, &name, bytes.len() as u64);
        // The row: ordinary chain send (or fleet-forward on a chainless device) — everything downstream treats it as a normal message.
        if !self.send_chain_message(ci, &content, false, None, None) {
            crate::log("attach: row send failed (no chain, no fleet) — attachment stays local");
        }
        // The blob: eager PT push to the friend. Siblings + offline races fetch on demand (attach_req).
        self.send_attach_blob(ci, &hash);
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
            .any(|c| c.is_sibling && c.public_identity.key == *device);
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
            (c.public_identity.key, c.race_addrs(), relay_to, token)
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

    /// Ask for a missing blob: attach_req to the conversation's friend device AND every online sibling — whoever holds it answers with an attach_blob.
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
        let Ok(vsf_bytes) = crate::network::fgtw::protocol::build_attach_req_vsf(
            &token,
            content_hash,
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
                targets.push((a, alt, c.public_identity.key, relay));
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
        let plain = crate::storage::blob_load(&seed, content_hash)?;
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
        std::fs::write(&dest, &plain).ok()?;
        Some(dest.to_string_lossy().into_owned())
    }

    /// Drain completed peer-avatar downloads: colour-convert the VSF-RGB pixels to the display buffer (same path as the self avatar) and install them on the matching contact, invalidating its scaled cache so the next render rebuilds + shows it. A `None` result (no avatar / fetch failed) just leaves the placeholder.
    /// Drain attachment blobs a worker verified + stored off-thread: send the attach_have confirm (needs the keypair + checker, which is why it can't run in the worker) so the pusher's pill flips to delivered, then clear the compose wrap and repaint.
    pub(super) fn drain_attach_installed(&mut self) {
        while let Ok(r) = self.attach_installed_rx.try_recv() {
            crate::logf!("ATTACH: blob received + stored ({} bytes)", r.len);
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
