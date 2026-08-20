//! Fleet device management — add-device/join flows, roster sync, fleet key custody, checkpoints, lockout, log submission, and reseeding the derived key sets.

use super::*;

impl PhotonApp {
    pub(super) fn open_add_device_flow(&mut self) {
        self.add_device_candidates.clear();
        self.add_device_bound = None;
        self.add_device_bind_ble = false;
        self.add_device_typo = None;
        self.add_device_wordcheck_text.clear();
        self.add_device_checking = false;
        self.add_device_status = "Type the words shown on the new device".to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        self.add_device_rx = Some(rx);
        self.add_device_tx = Some(tx);
        self.state = AppState::AddDevice;
        if let Some(tb) = self.textbox.as_mut() {
            tb.clear();
        }
        let tb_id = self.textbox.as_ref().map(|t| t.hit_id());
        self.change_focus(tb_id);
        self.spawn_bindreq_watch();
    }

    /// Off-thread candidate watch for the AddDevice screen: list the registry (member-gated, signature-verified in the client), push the set up, then wait on a hub poke (`pair_evt` kind "request") or an ~8s cadence — whichever first — until the flow's stop flag drops it. Push-driven with a poll floor, the join loop's shape.
    pub(super) fn spawn_bindreq_watch(&mut self) {
        use crate::network::fgtw::fleet;
        use std::sync::atomic::{AtomicBool, Ordering};
        let (Some(hp), Some(kp), Some(seed), Some(tx)) = (
            self.our_handle_proof(),
            self.device_keypair.clone(),
            self.session.as_ref().map(|s| s.identity_seed),
            self.add_device_tx.clone(),
        ) else {
            return;
        };
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        self.add_device_stop = Some(stop.clone());
        std::thread::spawn(move || {
            // Hub subscription: a "request" event for OUR identity pokes the refetch the moment the new device posts/withdraws; the poll cadence carries when the socket is down. Best-effort accelerator, never load-bearing.
            let (wake_tx, wake_rx) = std::sync::mpsc::channel::<()>();
            {
                let stop = stop.clone();
                crate::network::http::runtime().spawn(async move {
                    use futures::StreamExt;
                    let Ok((mut ws, _)) = tokio_tungstenite::connect_async(crate::network::http::SEED_WS).await else {
                        return;
                    };
                    loop {
                        tokio::select! {
                            frame = ws.next() => {
                                match frame {
                                    Some(Ok(m)) => {
                                        if stop.load(Ordering::Relaxed) {
                                            return;
                                        }
                                        if let Some((kind, evt_hp)) = fleet::parse_pair_event(&m.into_data()) {
                                            if evt_hp == hp && kind == "request" {
                                                let _ = wake_tx.send(());
                                            }
                                        }
                                    }
                                    _ => return, // closed or errored — poll cadence takes over
                                }
                            }
                            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                                if stop.load(Ordering::Relaxed) {
                                    return;
                                }
                            }
                        }
                    }
                });
            }
            loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                match fleet::bindreq_list(&kp, &hp, &seed) {
                    Ok(reqs) => {
                        let _ = tx.send(AddDeviceUpdate::Candidates(reqs));
                    }
                    Err(e) => crate::logf!("FLEET: bindreq list failed: {}", e),
                }
                match wake_rx.recv_timeout(crate::jitter_dur(std::time::Duration::from_secs(8))) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        std::thread::sleep(crate::jitter_dur(std::time::Duration::from_secs(8)));
                    }
                }
            }
        });
    }

    /// Re-derive the AddDevice status from the current entry + candidate set — the live matcher (docs/pairing-v2.md). Per candidate: every completed typed token must equal the expected token exactly; the still-being-typed last token passes while it's a prefix of the candidate's next word. Divergence flags the exact word; a full 23-token exact match IS the selection and auto-binds (correct words are the confirmation — the fleet key still waits behind the green confirm).
    pub(super) fn refresh_add_device_match(&mut self) {
        use crate::network::fgtw::fleet;
        if !matches!(self.state, AppState::AddDevice)
            || self.add_device_bound.is_some()
            || self.add_device_checking
        {
            return;
        }
        let text = self.add_device_wordcheck_text.clone();
        let tokens = fleet::pair_word_list(&text);
        self.add_device_typo = None;
        if self.add_device_candidates.is_empty() {
            // Nothing to match against yet — the voca spell-check still catches finger trouble while the registry answers.
            self.add_device_typo = fleet::first_bad_pair_word(&text);
            self.add_device_status = match &self.add_device_typo {
                Some(w) => format!("'{w}' isn't one of the words"),
                None => {
                    "Waiting for the new device\u{2026} it should be showing its words".to_string()
                }
            };
            return;
        }
        if tokens.is_empty() {
            let n = self.add_device_candidates.len();
            self.add_device_status = if n == 1 {
                "Type the words shown on the new device".to_string()
            } else {
                format!(
                    "{n} devices asking to join \u{2014} type the words on the one in your hand"
                )
            };
            return;
        }
        // The last typed token is COMPLETE (must match exactly) only once a separator follows it — same rule as first_bad_pair_word, so nothing flashes red mid-word.
        let last_complete = text != text.trim_end();
        let n = tokens.len();
        let mut full_match: Option<usize> = None;
        let mut alive = 0usize;
        let mut alive_idx = 0usize;
        let mut deepest = 0usize;
        for (ci, cand) in self.add_device_candidates.iter().enumerate() {
            let mut depth = 0usize;
            let mut ok = n <= cand.tokens.len();
            if ok {
                for (i, t) in tokens.iter().enumerate() {
                    let is_last = i + 1 == n;
                    let hit = if !is_last || last_complete {
                        &cand.tokens[i] == t
                    } else {
                        cand.tokens[i].starts_with(t.as_str())
                    };
                    if hit {
                        depth += 1;
                    } else {
                        ok = false;
                        break;
                    }
                }
            }
            deepest = deepest.max(depth);
            if ok {
                alive += 1;
                alive_idx = ci;
                if n == fleet::PAIR_WORD_COUNT && tokens[n - 1] == cand.tokens[n - 1] {
                    full_match = Some(ci);
                }
            }
        }
        if let Some(ci) = full_match {
            // Exact 23-word match against a verified request — the selection is made. Bind (phase one); the fold re-verifies the consent this request carries. WORDS path → auto-rotate (clear any stale BLE-tap flag so the Bound handler doesn't park on the confirm).
            let req = self.add_device_candidates[ci].req.clone();
            self.add_device_bind_ble = false;
            self.spawn_bind_device(req);
            return;
        }
        if alive == 0 {
            // The entry left every candidate at token index `deepest` — name that exact word in red.
            let bad = tokens.get(deepest.min(n - 1)).cloned().unwrap_or_default();
            self.add_device_status = format!("'{bad}' doesn't match any device asking to join");
            self.add_device_typo = Some(bad);
        } else if alive == 1 {
            let name = &self.add_device_candidates[alive_idx].name;
            self.add_device_status = format!("matching {name}\u{2026}");
        } else {
            self.add_device_status = format!("matching\u{2026} ({alive} devices)");
        }
    }

    /// Stop the NEW-device join thread and clear its display state (words, ready light, handle step).
    pub(super) fn end_join_flow(&mut self) {
        if let Some(stop) = self.add_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.add_join_handle = None;
        self.add_join_rx = None;
        self.add_join_words = None;
    }

    /// The fleet key from the local vault cache (fast, no network, no mint). `None` until a fan-out recover/establish has populated it (`spawn_fleet_key_sync`). Callers that seal/open fleet state read this; the background sync keeps it fresh so a rotation propagates.
    pub(super) fn fleet_key_cached(&self) -> Option<[u8; 32]> {
        // RAM first — the vault read below stalls the UI thread behind kete write commits during history storms; it now runs only until the first hit, and key writers refresh the RAM copy on every rotation/sync.
        if let Some(k) = self.fleet_key_ram.lock().ok().and_then(|g| *g) {
            return Some(k);
        }
        let storage = self.storage.as_ref()?;
        let session = self.session.as_ref()?;
        let addr = crate::storage::vault_key("fleet_key", &session.vault_seed);
        if let Ok(Some(bytes)) = storage.read_addr(&addr) {
            if let Ok(k) = <[u8; 32]>::try_from(bytes.as_slice()) {
                if let Ok(mut g) = self.fleet_key_ram.lock() {
                    *g = Some(k);
                }
                return Some(k);
            }
        }
        None
    }

    /// Load the epoch spine from the vault into RAM (idempotent; called from the fleet sweep until it lands).
    pub(super) fn fleet_epoch_load(&mut self) {
        if self.fleet_epoch.is_some() || self.ckpt_loaded {
            return;
        }
        let (Some(storage), Some(session)) = (self.storage.as_ref(), self.session.as_ref()) else {
            return;
        };
        self.ckpt_loaded = true;
        let addr = crate::storage::vault_key("fleet_epoch", &session.vault_seed);
        if let Ok(Some(bytes)) = storage.read_addr(&addr) {
            if let Some((k, epoch, prev)) = crate::network::fgtw::fleet::ckpt_state_decode(&bytes) {
                self.fleet_epoch = Some((k, epoch));
                self.fleet_epoch_prev = prev;
                // The vault-loaded spine must reach the pong map too — the boot reseed ran pre-load and derived v0 static keys.
                self.reseed_pong_seal_keys();
            }
        }
    }

    /// Persist the epoch spine to the vault (same bytes the custody slot and ckpt_state frames carry).
    pub(super) fn fleet_epoch_store(&self) {
        let (Some(storage), Some(session), Some((k, epoch))) = (
            self.storage.as_ref(),
            self.session.as_ref(),
            self.fleet_epoch,
        ) else {
            return;
        };
        let addr = crate::storage::vault_key("fleet_epoch", &session.vault_seed);
        let bytes = crate::network::fgtw::fleet::ckpt_state_bytes(k, &epoch, self.fleet_epoch_prev);
        if let Err(e) = storage.write_addr(&addr, &bytes) {
            crate::logf!("CKPT: epoch spine store failed: {}", e);
        }
    }

    /// The fleet-wide settled root over every non-sibling conversation's rows in RAM: per-conversation merkle over (eagle_time, content) leaves in (stamp, leaf) order, conversations bound in token order. Sibling 1:1 chatter is device-pair-local and deliberately outside the root — the root must be derivable identically fleet-wide.
    pub(super) fn settled_root_now(&self) -> [u8; 32] {
        use crate::crypto::clutch::{ckpt_conv_root, ckpt_leaf, ckpt_settled_root};
        let mut convs: Vec<([u8; 32], [u8; 32])> = Vec::new();
        for ci in 0..self.contacts.len() {
            if self.contacts[ci].is_sibling {
                continue;
            }
            let Some(conv) = self.conv_of(ci) else {
                continue;
            };
            let mut leaves: Vec<(i64, [u8; 32])> = conv
                .messages
                .iter()
                .filter(|m| !crate::types::is_control_content(&m.content))
                .map(|m| (m.timestamp, ckpt_leaf(m.timestamp, &m.content)))
                .collect();
            leaves.sort();
            convs.push((
                *conv.id().as_bytes(),
                ckpt_conv_root(leaves.into_iter().map(|(_, l)| l).collect()),
            ));
        }
        convs.sort();
        ckpt_settled_root(&convs)
    }

    /// Ship one fleet-scoped frame to every non-locked sibling (direct legs race, relay covers the rest) — the ckpt frames' transport, same targeting as the chain_sync push.
    pub(super) fn dispatch_frame_to_siblings(&self, frame: Vec<u8>) {
        let Some(checker) = self.status_checker.as_ref() else {
            return;
        };
        let unspecified = std::net::SocketAddr::from(([0, 0, 0, 0], 0));
        let dispatch = checker.history_dispatch();
        for sib in self
            .contacts
            .iter()
            .filter(|c| c.is_sibling && !c.locked_out)
        {
            let (primary, alt, relay_to) = match sib.race_addrs() {
                Some((p, a)) => (
                    p,
                    a,
                    relay_unless_direct_trusted(&sib, crate::network::udp::get_local_ip()),
                ),
                None => {
                    let relays = sib.relay_device_list();
                    if relays.is_empty() {
                        continue;
                    }
                    (unspecified, None, relays)
                }
            };
            let _ = dispatch.send(crate::network::status::HistorySendRequest {
                peer_addr: primary,
                alt_addr: alt,
                recipient_pubkey: *sib.public_identity.as_bytes(),
                relay_to,
                vsf_bytes: frame.clone(),
            });
        }
    }

    /// The checkpoint spine driver, run from the fleet sweep: lazy vault load, mint-outcome drain, and the two mint edges (no spine yet → bootstrap k=1; rotation flagged → advance). Off-thread for everything that touches the network; single-flight via `ckpt_busy`.
    pub(super) fn ckpt_tick(&mut self) {
        self.fleet_epoch_load();
        // Drain the off-thread outcome first — a landed advance clears busy and may satisfy the pending edge.
        if let Some(rx) = self.ckpt_rx.as_ref() {
            match rx.try_recv() {
                Ok(CkptOutcome::Advanced {
                    k,
                    epoch,
                    prev,
                    root,
                    fanout_epoch,
                    minted_here,
                }) => {
                    self.ckpt_busy = false;
                    self.ckpt_rx = None;
                    self.ckpt_spineless_holds = 0;
                    self.fleet_epoch_prev = prev;
                    self.fleet_epoch = Some((k, epoch));
                    self.fleet_epoch_store();
                    // Sibling pong tails ride the epoch key — flip ours on the same edge the spine advances so both ends of every pair converge on the k key together.
                    self.reseed_pong_seal_keys();
                    crate::logf!(
                        "CKPT: epoch spine at k={} ({})",
                        k,
                        if minted_here {
                            "minted here"
                        } else {
                            "adopted"
                        }
                    );
                    // The winner hands the SECRET root to siblings current at k−1; they verify by open-success and derive. k=1 has no prior epoch anywhere — siblings jump via ckpt_state instead.
                    if minted_here {
                        if let (Some((root, fe)), Some((_, prev_epoch)), Some(kp)) = (
                            root.map(|r| (r, fanout_epoch)),
                            prev,
                            self.device_keypair.as_ref(),
                        ) {
                            let seal_key = crate::crypto::clutch::fleet_epoch_seal_key(
                                &prev_epoch,
                                b"ckpt_root",
                            );
                            if let Ok(sealed) = kete::encrypt_bytes(&root, &seal_key) {
                                if let Ok(frame) =
                                    crate::network::fgtw::protocol::build_ckpt_root_vsf(
                                        k,
                                        fe,
                                        sealed,
                                        kp.public.as_bytes(),
                                        kp.secret.as_bytes(),
                                    )
                                {
                                    self.dispatch_frame_to_siblings(frame);
                                }
                            }
                        }
                    }
                }
                Ok(CkptOutcome::SpinelessHold) => {
                    self.ckpt_busy = false;
                    self.ckpt_rx = None;
                    self.ckpt_spineless_holds = self.ckpt_spineless_holds.saturating_add(1);
                    // Ask every sibling for its spine state — the intended jump path when SOMEONE holds it. The counter is what breaks the mutual-spineless case (all holders wiped/rotated-past): three dry sweeps arm the supersession in the next tick's spawn.
                    crate::logf!(
                        "CKPT: spineless hold #{} — requesting ckpt_state from siblings{}",
                        self.ckpt_spineless_holds,
                        if self.ckpt_spineless_holds >= 3 {
                            " (supersession armed: nobody alive can read the old spine)"
                        } else {
                            ""
                        }
                    );
                    if let Some(kp) = self.device_keypair.as_ref() {
                        if let Ok(frame) = crate::network::fgtw::protocol::build_ckpt_req_vsf(
                            0,
                            kp.public.as_bytes(),
                            kp.secret.as_bytes(),
                        ) {
                            self.dispatch_frame_to_siblings(frame);
                        }
                    }
                }
                Ok(CkptOutcome::Idle) => {
                    self.ckpt_busy = false;
                    self.ckpt_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.ckpt_busy = false;
                    self.ckpt_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.ckpt_busy {
            return;
        }
        // The row-cadence dial (braid.md §14.3/§14.4): total syncable rows (cheap — a len() sum, no hashing) against the base at the last cadence edge; growth past CKPT_ROW_CADENCE flags a mint so the burn horizon tracks traffic. First observation seeds the base — a restart never mints spuriously. The base advances when the flag raises, so a lost mint race just rides the adopted spine and the next cadence measures from here.
        const CKPT_ROW_CADENCE: usize = 128;
        let rows_now: usize = (0..self.contacts.len())
            .filter(|&ci| !self.contacts[ci].is_sibling)
            .filter_map(|ci| self.conv_of(ci))
            .map(|c| c.messages.len())
            .sum();
        match self.ckpt_rows_base {
            None => self.ckpt_rows_base = Some(rows_now),
            Some(base) if rows_now >= base + CKPT_ROW_CADENCE => {
                crate::logf!(
                    "CKPT: {} fresh row(s) since the last cadence edge — minting",
                    rows_now - base
                );
                self.ckpt_mint_due = true;
                self.ckpt_rows_base = Some(rows_now);
            }
            _ => {}
        }
        let bootstrap = self.fleet_epoch.is_none();
        if !bootstrap && !self.ckpt_mint_due {
            return;
        }
        // Sweep-cadence bound: a wedged bootstrap (custody unreadable, fgtw down) must not become a per-frame fetch storm.
        if self
            .ckpt_last_attempt
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(45))
        {
            return;
        }
        self.ckpt_last_attempt = Some(Instant::now());
        let (Some(kp), Some(hp), Some(fleet_key), Some(storage)) = (
            self.device_keypair.clone(),
            self.our_handle_proof(),
            self.fleet_key_cached(),
            self.storage.as_ref().cloned(),
        ) else {
            return;
        };
        self.ckpt_mint_due = false;
        self.ckpt_busy = true;
        let root = self.settled_root_now();
        let cur = self.fleet_epoch;
        // Third dry spineless sweep arms the breaker: this spawn may supersede the unreadable spine.
        let supersede = self.ckpt_spineless_holds >= 3;
        let (tx, rx) = std::sync::mpsc::channel();
        self.ckpt_rx = Some(rx);
        std::thread::spawn(move || {
            use crate::network::fgtw::fleet;
            let send = |o: CkptOutcome| {
                let _ = tx.send(o);
            };
            // The fan-out epoch names WHICH fleet key folds into the derivation — recover the pair together so they can't skew.
            let (fanout_epoch, fk) =
                match fleet::recover_fleet_key_with_epoch(&hp, &kp, Some(&storage)) {
                    Ok(Some((e, k))) => (e, k),
                    _ => (0, fleet_key),
                };
            if let Some((have_k, have_epoch)) = cur {
                // Steady-state advance (rotation edge): push k+1 folding the fresh key; a loss means a sibling minted — its ckpt_root frame carries the root home.
                let commit = crate::crypto::clutch::ckpt_commit(&root);
                match fleet::push_checkpoint(&hp, &kp, have_k + 1, commit, fanout_epoch) {
                    Ok(None) => {
                        let epoch = crate::crypto::clutch::advance_fleet_epoch(
                            &have_epoch,
                            &root,
                            &fk,
                            fanout_epoch,
                            have_k + 1,
                        );
                        let state =
                            fleet::ckpt_state_bytes(have_k + 1, &epoch, Some((have_k, have_epoch)));
                        if let Err(e) = fleet::ckpt_custody_write(&fk, &state, &kp, &hp) {
                            crate::logf!(
                                "CKPT: custody write failed (spine still advanced): {}",
                                e
                            );
                        }
                        send(CkptOutcome::Advanced {
                            k: have_k + 1,
                            epoch,
                            prev: Some((have_k, have_epoch)),
                            root: Some(root),
                            fanout_epoch,
                            minted_here: true,
                        });
                    }
                    Ok(Some((wk, _, _))) => {
                        crate::logf!("CKPT: lost the k={} race to a sibling (chain holds k={}) — adopting via its root hand-off", have_k + 1, wk);
                        send(CkptOutcome::Idle);
                    }
                    Err(e) => {
                        crate::logf!(
                            "CKPT: checkpoint push failed (re-arms on the next edge): {}",
                            e
                        );
                        send(CkptOutcome::Idle);
                    }
                }
                return;
            }
            // Bootstrap: no spine here. Custody first (a sibling may have minted), else mint epoch 0 (the 2MB reservoir collapse — the one-time fleet birth cost) and race k=1 on the chain.
            if let Some(bytes) = fleet::ckpt_custody_read(&fk) {
                if let Some((k, epoch, prev)) = fleet::ckpt_state_decode(&bytes) {
                    send(CkptOutcome::Advanced {
                        k,
                        epoch,
                        prev,
                        root: None,
                        fanout_epoch,
                        minted_here: false,
                    });
                    return;
                }
            }
            let (genesis_hash, chain_k) = match fleet::fetch(&hp) {
                Ok(Some(blob)) => {
                    if let Some((ck, _, _)) = blob.latest_checkpoint() {
                        if !supersede {
                            // The chain has a spine but custody didn't open (rotation gap, or the winner is mid-write) — the drain fires a ckpt_req and counts this sweep toward the supersession breaker.
                            crate::log("CKPT: chain has a spine but custody is unreadable yet — holding for ckpt_state");
                            send(CkptOutcome::SpinelessHold);
                            return;
                        }
                        // SUPERSESSION: the chain names a spine NOBODY ALIVE can read — its minter was wiped and the fleet key rotated past its custody seal, so waiting is a permanent wedge with all fleet-plane traffic (chain_sync, fleet hist_pages) held behind the missing spine (field, 2026-08-16). A spine is derivable state, not data: mint a fresh epoch seed and push chain_k+1; the chain's monotonic guard picks exactly one winner, whose custody write (under the CURRENT fleet key) unwedges every sibling on its next sweep. A returning device that still holds the old spine adopts the newer k via ckpt_state like any lagger.
                        crate::logf!("CKPT: superseding the unreadable spine at chain k={} — re-minting at k={}", ck, ck + 1);
                        (blob.genesis_hash().unwrap_or([0u8; 32]), ck)
                    } else {
                        (blob.genesis_hash().unwrap_or([0u8; 32]), 0)
                    }
                }
                _ => {
                    send(CkptOutcome::Idle);
                    return;
                }
            };
            let epoch_zero = crate::crypto::clutch::mint_fleet_epoch_seed(
                &kp.public.to_bytes(),
                &genesis_hash,
                &fk,
            );
            let commit = crate::crypto::clutch::ckpt_commit(&root);
            let new_k = chain_k + 1;
            match fleet::push_checkpoint(&hp, &kp, new_k, commit, fanout_epoch) {
                Ok(None) => {
                    let epoch = crate::crypto::clutch::advance_fleet_epoch(
                        &epoch_zero,
                        &root,
                        &fk,
                        fanout_epoch,
                        new_k,
                    );
                    let state = fleet::ckpt_state_bytes(new_k, &epoch, Some((chain_k, epoch_zero)));
                    if let Err(e) = fleet::ckpt_custody_write(&fk, &state, &kp, &hp) {
                        crate::logf!("CKPT: bootstrap custody write failed (siblings jump via ckpt_state): {}", e);
                    }
                    crate::logf!("CKPT: fleet epoch spine {} — reservoir minted, k={} on the chain", if chain_k == 0 { "born" } else { "SUPERSEDED" }, new_k);
                    // No root broadcast: nobody else holds the fresh epoch seed, so siblings jump via ckpt_state/custody.
                    send(CkptOutcome::Advanced {
                        k: new_k,
                        epoch,
                        prev: Some((chain_k, epoch_zero)),
                        root: None,
                        fanout_epoch,
                        minted_here: true,
                    });
                }
                Ok(Some((wk, _, _))) => {
                    crate::logf!("CKPT: bootstrap race lost (chain holds k={}) — adopting the winner's custody next sweep", wk);
                    send(CkptOutcome::Idle);
                }
                Err(e) => {
                    crate::logf!("CKPT: bootstrap checkpoint push failed: {}", e);
                    send(CkptOutcome::Idle);
                }
            }
        });
    }

    /// Recover the current fleet key from the fan-out (or establish it at genesis), off-thread, and cache it in the vault. This is how a device gets the fleet key now — sealed to its own device key, recoverable with just its `ihi` — superseding the pairing-secret hand-off. Triggered on attest and after a membership change, so a rotation propagates to every device. Session-long subscription to the FGTW hub's typed events for OUR identity. "fstate" (roster changed by a sibling device) and "fleet" (membership chain extended) land in `fleet_evt_rx`; tick drains them into a roster pull / key sync, and the wake proxy pokes the loop so it reacts immediately. Reconnects with jittered backoff — unlike the join ceremony's throwaway socket, this one is the LIVE propagation path (friend added on one device appears on the others), so it survives network blips. Idempotent: repeat calls while a subscription is live are no-ops.
    pub(super) fn spawn_fleet_event_sub(&mut self) {
        use std::sync::atomic::Ordering;
        if self.fleet_evt_rx.is_some() {
            return;
        }
        let Some(hp) = self.our_handle_proof() else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel::<(&'static str, [u8; 32])>();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.fleet_evt_rx = Some(rx);
        self.fleet_evt_stop = Some(stop.clone());
        let wake = self.event_proxy.clone();
        let _ = hp; // attest-gate only; we no longer filter by our own hp — tick routes by hp (ours vs a contact's)
        crate::network::http::runtime().spawn(async move {
            use futures::StreamExt;
            loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                if let Ok((mut ws, _)) = tokio_tungstenite::connect_async(crate::network::http::SEED_WS).await {
                    loop {
                        tokio::select! {
                            frame = ws.next() => {
                                match frame {
                                    Some(Ok(m)) => {
                                        if stop.load(Ordering::Relaxed) {
                                            return;
                                        }
                                        // Forward EVERY recognized event with its hp — no our-hp filter. Registry dings (fleet) go photon-wide, so a contact's chain change reaches us here too; tick decides whether an hp is ours (roster/key sync) or a contact's (member refresh). Bumps are tiny and the hub is low-traffic, so receive-all + filter-app-side is cheaper than re-subscribing every time contacts change.
                                        if let Some((kind, evt_hp)) = crate::network::fgtw::fleet::parse_pair_event(&m.into_data()) {
                                            let k: &'static str = match kind.as_str() {
                                                "fstate" => "fstate",
                                                "fleet" => "fleet",
                                                "friendship" => "friendship",
                                                "release" => "release",
                                                _ => continue,
                                            };
                                            if tx.send((k, evt_hp)).is_err() {
                                                return; // app side dropped the receiver — subscription retired
                                            }
                                            if let Some(w) = wake.as_ref() {
                                                let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
                                            }
                                        }
                                    }
                                    _ => break, // closed / errored — fall out to the reconnect sleep
                                }
                            }
                            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                                if stop.load(Ordering::Relaxed) {
                                    return;
                                }
                            }
                        }
                    }
                }
                tokio::time::sleep(crate::jitter_dur(std::time::Duration::from_secs(20))).await;
            }
        });
        crate::log("FLEET: event subscription started (fstate/fleet/friendship push)");
    }

    /// Off-thread: fold each given contact's public membership chain (`fleet::current_members_with_ts` by handle_proof, blocking HTTP) and post back the current device set plus the chain-tip eagle time for tick to adopt into `fleet_members` (monotonically — see the drain). Idempotent and best-effort: a fetch failure sends nothing, so the drain never runs and the last-known folded set + arm flag stay untouched (never trust-nobody on a network blip). Results land via `contact_members_rx`. Started on contact load/merge (wake-up catch-up) and on a `fleet` bump for a contact's identity (live).
    pub(super) fn spawn_contact_fleet_refresh(&mut self, handle_proofs: Vec<[u8; 32]>) {
        if handle_proofs.is_empty() {
            return;
        }
        if self.contact_members_tx.is_none() {
            let (tx, rx) =
                std::sync::mpsc::channel::<([u8; 32], Vec<[u8; 32]>, i64, [u8; 32], bool)>();
            self.contact_members_rx = Some(rx);
            self.contact_members_tx = Some(tx);
        }
        let tx = self.contact_members_tx.as_ref().unwrap().clone();
        let wake = self.event_proxy.clone();
        std::thread::spawn(move || {
            for hp in handle_proofs {
                match crate::network::fgtw::fleet::current_members_full(&hp) {
                    Ok((members, tip_ts, genesis, existed)) => {
                        if tx.send((hp, members, tip_ts, genesis, existed)).is_err() {
                            return; // app dropped the receiver
                        }
                    }
                    Err(e) => crate::logf!(
                        "FLEET: contact fleet refresh failed for {}: {}",
                        crate::fp(&hp),
                        e
                    ),
                }
            }
            if let Some(w) = wake.as_ref() {
                let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
            }
        });
    }

    /// Off-thread: a contact's fold arrived with a genesis DIFFERENT from its pinned one — probe for a succession record that traces back to the pin (docs/identity-succession.md). Fetches the (public) successor slot and runs `verify_for_pin` against `pinned_genesis` HERE, off the UI thread — only a record whose continuity egg is signed by a PREDECESSOR-chain member over the exact transition verifies, so a handle-only impostor cannot forge a re-pin. Posts `(hp, Some(new_genesis))` on success (tick migrates the pin), `(hp, None)` otherwise (tick just clears the in-flight guard so a later refresh re-probes). Results land via `successor_rx`.
    pub(super) fn spawn_successor_check(&mut self, handle_proof: [u8; 32], pinned_genesis: [u8; 32]) {
        if self.successor_tx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel::<([u8; 32], Option<[u8; 32]>)>();
            self.successor_rx = Some(rx);
            self.successor_tx = Some(tx);
        }
        let tx = self.successor_tx.as_ref().unwrap().clone();
        let wake = self.event_proxy.clone();
        self.successor_inflight.insert(handle_proof);
        std::thread::spawn(move || {
            let outcome = match crate::network::fgtw::fleet::fetch_successor(&handle_proof) {
                Ok(Some(record)) => match record.verify_for_pin(&pinned_genesis) {
                    Ok(()) => Some(record.new_genesis_hash),
                    Err(e) => {
                        crate::logf!(
                            "SUCCESSION: {} record did not verify against the pin: {}",
                            crate::fp(&handle_proof),
                            e
                        );
                        None
                    }
                },
                Ok(None) => None, // no record published (yet)
                Err(e) => {
                    crate::logf!(
                        "SUCCESSION: fetch failed for {}: {}",
                        crate::fp(&handle_proof),
                        e
                    );
                    None
                }
            };
            if tx.send((handle_proof, outcome)).is_err() {
                return; // app dropped the receiver
            }
            if let Some(w) = wake.as_ref() {
                let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
            }
        });
    }

    /// Reconcile OUR OWN folded fleet membership into sibling contacts (the fleet weave). For each member device that isn't us and has no sibling contact yet: create one as `ClutchState::Pending` — the serialized keygen queue picks it up and the full CLUTCH ceremony + weave runs against it exactly like a friend. For each sibling contact whose device fell out of the fold: remove it and delete its state + chains (revocation hygiene — an ex-member must not stay ceremony-eligible). Idempotent; triggered from attest/resume, our-hp `fleet` events, and the binder's `AddDeviceUpdate::Bound`.
    pub(super) fn reconcile_fleet_siblings(&mut self, members: &[[u8; 32]]) {
        let Some(our_device) = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes()) else {
            return;
        };
        let Some(our_hp) = self.our_handle_proof() else {
            return;
        };
        let mut changed = false;
        let mut added_sibling = false;

        // Add newly-folded members.
        for device in members {
            if *device == our_device {
                continue;
            }
            if self
                .contacts
                .iter()
                .any(|c| c.is_sibling && c.public_identity.key == *device)
            {
                continue;
            }
            let sib = crate::types::Contact::new_sibling(
                our_hp,
                crate::types::DevicePubkey::from_bytes(*device),
            );
            crate::logf!(
                "SIBLING: reconciled +1 (device {}) — fleet weave pending",
                hex::encode(&device[..4])
            );
            if let Some(storage) = self.storage.as_ref() {
                if let Err(e) = crate::storage::contacts::save_contact(&sib, storage) {
                    crate::logf!("SIBLING: failed to persist sibling: {}", e);
                }
            }
            self.contacts.push(sib);
            changed = true;
            added_sibling = true;
        }

        // Drop de-folded members (device removed from OUR chain).
        let mut removed: Vec<crate::types::Contact> = Vec::new();
        self.contacts.retain(|c| {
            if c.is_sibling && !members.contains(&c.public_identity.key) {
                removed.push(c.clone());
                false
            } else {
                true
            }
        });
        for c in &removed {
            crate::logf!(
                "SIBLING: reconciled -1 (device {} left the fold)",
                hex::encode(&c.public_identity.key[..4])
            );
            changed = true;
            if let Some(storage) = self.storage.as_ref() {
                if let Err(e) =
                    crate::storage::contacts::delete_sibling(&c.public_identity.key, storage)
                {
                    crate::logf!("SIBLING: failed to delete sibling state: {}", e);
                }
                if let Some(fid) = c.friendship_id {
                    self.friendship_chains.retain(|(id, _)| *id != fid);
                    if let Err(e) =
                        crate::storage::friendship::delete_friendship_chains(&fid, storage)
                    {
                        crate::logf!("SIBLING: failed to delete sibling chains: {}", e);
                    }
                }
            }
        }
        if !removed.is_empty() {
            // Removal rotates (braid.md §14.2): a shrink in OUR OWN fold is the live trigger — any surviving member mints the next epoch so the leaver's cached fleet key goes stale. The sentinel inside re-checks against server state (fold + fan-out), so a stale local cache can't force a bogus rotation.
            self.spawn_fleet_key_sync();
        }

        if changed {
            self.reseed_contact_pubkeys();
            // Freshly-created siblings start address-less; the stalled-contact pulse resolves their devices from the registry (the announce echo that used to address them here is gone — the worker acks without a peer list).
        }

        // A NEWLY-folded sibling has none of the fleet's chain state. Per-lane replication only sends lanes that ADVANCE, so an idle lane already far ahead (a peer device that sent a burst then went quiet) would never reach the new device, leaving it stuck at position 0 for that lane. Clear the per-lane + per-friendship push watermarks so the next drive_chain_replication pass re-pushes EVERY current lane checkpoint to the whole fleet — a one-time full catch-up on the join edge, the deltas resume after. Existing siblings no-op the re-adopt (no greater positions).
        if added_sibling {
            self.lane_pushed_pos.clear();
            self.chain_pushed_osc.clear();
        }
    }

    /// Mint the next fan-out epoch because a sibling pair just became EGGED (Phase A): until its pair secret existed the sibling could not be wrapped, so it held no fleet key. Same rotation the sponsor's confirm runs — the worker's monotonic epoch guard makes concurrent siblings converge, and a `stale` loser simply adopts the winner's key on its next sync.
    pub(super) fn spawn_fleet_key_rotate_for_compliance(&self) {
        use crate::network::fgtw::fleet;
        let (Some(hp), Some(device_key), Some(storage), Some(session)) = (
            self.our_handle_proof(),
            self.device_keypair.clone(),
            self.storage.as_ref().cloned(),
            self.session.as_ref(),
        ) else {
            return;
        };
        let identity_seed = session.identity_seed;
        let addr = crate::storage::vault_key("fleet_key", &session.vault_seed);
        let busy = self.fleet_heal_busy.clone();
        let fkc = self.fleet_key_ram.clone();
        // Snapshot on the UI thread, filter in the closure — same discipline as the heal. Unfiltered members here RE-WRAPPED a locked device at the next new-egg rotation, silently undoing the lock's key rotation (found 2026-08-15 wiring unlock).
        let locked = self.locked_devices();
        std::thread::spawn(move || {
            // RAII latch: released on EVERY exit, panics included — a manual store(false) here once left a poisoned latch parking key sync for the whole session.
            let Some(_latch) = HealLatch::acquire(&busy) else {
                return; // a heal already holds the latch; its rotation covers this too
            };
            // Genesis-verified fold: never rotate toward a member set a relay could have swapped.
            let members = match fleet::current_members_verified(&hp) {
                Ok(m) if !m.is_empty() => m
                    .into_iter()
                    .filter(|m| !locked.contains(m))
                    .collect::<Vec<_>>(),
                _ => return,
            };
            if members.is_empty() {
                return;
            }
            match fleet::rotate_fleet_key(&hp, &device_key, &members, Some(&storage)) {
                Ok((epoch, k)) => {
                    if let Err(e) = storage.write_addr(&addr, &k) {
                        crate::logf!("FANOUT: fleet key cache failed: {}", e);
                    }
                    if let Ok(mut g) = fkc.lock() {
                        *g = Some(k);
                    }
                    // Every key edge refreshes OUR wipe-proof recovery slot — the path a wiped, sibling-less device gets its contacts back thru.
                    fleet::publish_recovery_slot(&k, &identity_seed, &device_key, &hp);
                    crate::logf!(
                        "FANOUT: rotated to epoch {} — egged siblings wrapped",
                        epoch
                    );
                }
                Err(e) => crate::logf!("FANOUT: compliance rotation failed: {}", e),
            }
        });
    }

    /// Also the removal-rotates sentinel (braid.md §14.2 / §14.12-1): before recovering, the thread folds our chain (genesis-verified) and fetches the fan-out; a fan-out wrapping MORE devices than the fold holds means a departed member's wrap lingers, so this device — any surviving member, never the leaver — mints the next epoch and re-seals the fstate slot across the re-key (pull under the OLD cached key first, push the CRDT merge under the new; skipping the pull would let `push_roster`'s AEAD fallback silently drop the settings layer). One heal at a time (`fleet_heal_busy`, also parking plain syncs so they can't re-cache the pre-rotation key mid-heal); concurrent siblings converge on the worker's monotonic epoch guard — a `stale` loser adopts the winner's key and pushes nothing. The winner's UI-thread follow-up (avatar bearer-pin rotate) rides `fleet_rotated_tx`.
    pub(super) fn spawn_fleet_key_sync(&self) {
        use crate::network::fgtw::fleet;
        let (Some(hp), Some(device_key), Some(storage), Some(session)) = (
            self.our_handle_proof(),
            self.device_keypair.clone(),
            self.storage.as_ref().cloned(),
            self.session.as_ref(),
        ) else {
            return;
        };
        let identity_seed = session.identity_seed;
        let addr = crate::storage::vault_key("fleet_key", &session.vault_seed);
        let busy = self.fleet_heal_busy.clone();
        let fkc = self.fleet_key_ram.clone();
        let rotated_tx = self.fleet_rotated_tx.clone();
        let wake = self.event_proxy.clone();
        // Locked devices (treat-as-stolen) are chain members that must NOT be wrapped: the heal rotates whenever the live fan-out covers more devices than the DESIRED set (fold minus locked), which fires on a member's self-departure AND on a lockout identically.
        let locked = self.locked_devices();
        // This device's shareable state, snapshotted on the UI thread — the heal's re-seal push carries it even when the old-key pull comes up empty.
        let ours = fgtw::fstate::FleetState {
            roster: self.current_roster(),
            global_settings: self
                .fleet_settings
                .as_ref()
                .map(|fs| fs.global.clone())
                .unwrap_or_default(),
            device_settings: self
                .fleet_settings
                .as_ref()
                .map(|fs| fs.devices.clone())
                .unwrap_or_default(),
        };
        std::thread::spawn(move || {
            use std::sync::atomic::Ordering;
            if busy.load(Ordering::Acquire) {
                return; // a heal is mid-flight; it caches the freshest key itself, and its fanout_put fires a fresh `fleet` bump for a clean re-sync
            }
            // Fail toward no-rotate: any fetch/verify miss here just falls thru to the plain recover path.
            let heal_members = (|| {
                let members = fleet::current_members_verified(&hp).ok()?;
                let desired: Vec<[u8; 32]> = members
                    .into_iter()
                    .filter(|m| !locked.contains(m))
                    .collect();
                let (_, _, wraps) = fleet::fetch_fanout(&hp).ok().flatten()?;
                fleet::fanout_needs_rotation(wraps.len(), desired.len()).then_some(desired)
            })();
            if let Some(members) = heal_members {
                // RAII latch — released on every exit including a panic mid-rotate (the old manual store left it poisoned).
                let Some(_latch) = HealLatch::acquire(&busy) else {
                    return;
                };
                // Preserve the slot BEFORE the re-key makes it unopenable. A racing sibling's re-push can already have re-sealed it (AEAD miss → default) — then our push carries `ours` alone, and whatever the slot uniquely held rides back on that sibling's next write (CRDT union, no tombstone loss).
                let old_key = storage
                    .read_addr(&addr)
                    .ok()
                    .flatten()
                    .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok());
                let preserved = old_key
                    .and_then(|k| fleet::pull_fstate(&hp, &k).ok().flatten())
                    .unwrap_or_default();
                match fleet::rotate_fleet_key(&hp, &device_key, &members, Some(&storage)) {
                    Ok((epoch, new_key)) => {
                        if let Err(e) = storage.write_addr(&addr, &new_key) {
                            crate::logf!("FLEET: rotated key cache failed: {}", e);
                        }
                        if let Ok(mut g) = fkc.lock() {
                            *g = Some(new_key);
                        }
                        fleet::publish_recovery_slot(&new_key, &identity_seed, &device_key, &hp);
                        let merged = fgtw::fstate::merge_fstate(preserved, ours);
                        match fleet::push_fstate(&hp, &device_key, &new_key, &merged) {
                            Ok(()) => crate::logf!("FLEET: removal heal — rotated to epoch {} ({} member(s)), fstate re-sealed", epoch, members.len()),
                            Err(e) => crate::logf!("FLEET: removal heal — rotated to epoch {} but the re-seal push failed ({}); the next roster/settings write re-seals", epoch, e),
                        }
                        let _ = rotated_tx.send(());
                        if let Some(w) = wake.as_ref() {
                            let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
                        }
                    }
                    Err(e) => {
                        // Lost the race (or the write failed): a sibling's rotation is the live epoch — adopt it.
                        crate::logf!("FLEET: removal heal — rotation not ours ({}); adopting the current fan-out", e);
                        if let Ok(Some(k)) =
                            fleet::recover_fleet_key(&hp, &device_key, Some(&storage))
                        {
                            let _ = storage.write_addr(&addr, &k);
                            if let Ok(mut g) = fkc.lock() {
                                *g = Some(k);
                            }
                            fleet::publish_recovery_slot(&k, &identity_seed, &device_key, &hp);
                        }
                    }
                }
                return;
            }
            match fleet::recover_or_establish_fleet_key(&hp, &device_key, Some(&storage)) {
                Ok(Some(k)) => {
                    if let Err(e) = storage.write_addr(&addr, &k) {
                        crate::logf!("FLEET: fleet key cache failed: {}", e);
                    } else {
                        crate::log("FLEET: fleet key synced from fan-out");
                    }
                    if let Ok(mut g) = fkc.lock() {
                        *g = Some(k);
                    }
                    fleet::publish_recovery_slot(&k, &identity_seed, &device_key, &hp);
                }
                Ok(None) => {}
                Err(e) => crate::logf!("FLEET: fleet key sync failed: {}", e),
            }
        });
    }

    /// After binding a new device (which rotated the fan-out epoch), recover the CURRENT fleet key and re-seal our contact roster under it — in ONE ordered thread so the push can't race the async cache write and seal under the stale epoch. Without this the roster slot stays sealed under the pre-rotation key, and the freshly-joined device's pulls fail `aead::Error` forever (it correctly holds the new key). The roster entries are snapshotted on the UI thread (needs `&self`); the recover+cache+push run off-thread.
    pub(super) fn spawn_roster_republish(&self) {
        use crate::network::fgtw::fleet;
        let entries = self.current_roster();
        let (Some(hp), Some(kp), Some(storage), Some(session)) = (
            self.our_handle_proof(),
            self.device_keypair.clone(),
            self.storage.as_ref().cloned(),
            self.session.as_ref(),
        ) else {
            return;
        };
        let identity_seed = session.identity_seed;
        let addr = crate::storage::vault_key("fleet_key", &session.vault_seed);
        // Live settings ride every roster push — a race-losing push must never revert a value this device already holds (the boot-minted avatar pin, field 2026-08-02).
        let live = self
            .fleet_settings
            .as_ref()
            .map(|fs| (fs.global.clone(), fs.devices.clone()));
        let fkc = self.fleet_key_ram.clone();
        std::thread::spawn(move || {
            match fleet::recover_or_establish_fleet_key(&hp, &kp, Some(&storage)) {
                Ok(Some(k)) => {
                    if let Err(e) = storage.write_addr(&addr, &k) {
                        crate::logf!("FLEET: fleet key cache failed: {}", e);
                    }
                    if let Ok(mut g) = fkc.lock() {
                        *g = Some(k);
                    }
                    // Key edge → refresh the wipe-proof recovery slot.
                    fleet::publish_recovery_slot(&k, &identity_seed, &kp, &hp);
                    crate::log("FLEET: fleet key synced from fan-out (post-bind)");
                    if entries.is_empty() {
                        return; // no contacts to share, but the key is now current for the roster PULL
                    }
                    match fleet::push_roster_with_settings(&hp, &kp, &k, &entries, live) {
                        Ok(()) => crate::logf!(
                            "FLEET: roster re-pushed under rotated epoch ({} entr(ies))",
                            entries.len()
                        ),
                        Err(e) => crate::logf!("FLEET: roster re-push failed: {}", e),
                    }
                }
                Ok(None) => {}
                Err(e) => crate::logf!("FLEET: post-bind key sync failed: {}", e),
            }
        });
    }

    /// Persist a fleet key received over pairing (new device), overwriting any local placeholder so this device converges on the founder's key.
    pub(super) fn fleet_key_store(&self, key: &[u8; 32]) {
        if let (Some(storage), Some(session)) = (self.storage.as_ref(), self.session.as_ref()) {
            let addr = crate::storage::vault_key("fleet_key", &session.vault_seed);
            if let Err(e) = storage.write_addr(&addr, key) {
                crate::logf!("FLEET: fleet key store failed: {}", e);
            }
            if let Ok(mut g) = self.fleet_key_ram.lock() {
                *g = Some(*key);
            }
            // Key edge → refresh the wipe-proof recovery slot (off-thread: it is a blocking upload).
            if let (Some(kp), Some(hp)) = (self.device_keypair.clone(), self.our_handle_proof()) {
                let (k, seed) = (*key, session.identity_seed);
                std::thread::spawn(move || {
                    crate::network::fgtw::fleet::publish_recovery_slot(&k, &seed, &kp, &hp);
                });
            }
        }
    }

    /// Build the fleet roster from the live contact list — the syncable subset, minus fleet siblings (infrastructure, not friends — a sibling pid leaking into the roster would merge as a bogus contact on every device).
    pub(super) fn current_roster(&self) -> Vec<crate::network::fgtw::fleet::RosterEntry> {
        use crate::network::fgtw::fleet::RosterEntry;
        self.contacts
            .iter()
            // The SELF row rides the roster too. Excluding it meant notes-to-self was the one contact a wipe could never restore — you added it deliberately and then had to add it again, every time. It is a contact you chose, so it belongs in the thing that remembers your contacts; the keygen gate skips zero-remote conversations, so it never tries to run a ceremony against itself on arrival.
            .filter(|c| !c.is_sibling)
            .map(|c| RosterEntry {
                handle_proof: c.handle_proof,
                handle_hash: c.handle_hash,
                public_identity: *c.public_identity.as_bytes(),
                published_name: c.published_name.clone(),
                avatar_pin: c.avatar_pin,
                added: c.added,
                updated: c.roster_updated,
                tombstone: false,
                // §4.2 one-ceremony claim rides the entry. Woven carries whichever truth we hold: OUR chain woven (we're the owner) or the owner's roster-adopted completion.
                ceremony_owner: c.ceremony_owner.unwrap_or([0u8; 32]),
                woven: c.chain_woven || c.owner_woven,
                trust_level: crate::storage::cloud::trust_level_to_u8(c.trust_level),
            })
            .collect()
    }

    /// Merge pulled roster entries into the live contact list — the local half of the roster CRDT. New entries add as Pending stubs (then ONE serialized keygen re-CLUTCHes them; the tick loop does the rest one McEliece at a time so a multi-contact join doesn't starve the UI). For contacts we already hold, a strictly NEWER entry wins last-writer: its published_name/avatar_pin overwrite ours, and a newer tombstone removes the contact (state + index row; conversation content stays in rārangi — ostracism not erasure). Ties keep ours — the slot-side merge_rosters resolves exact ties deterministically, so pushing our copy converges.
    pub(super) fn merge_roster_entries(
        &mut self,
        entries: Vec<crate::network::fgtw::fleet::RosterEntry>,
    ) {
        let mut added = 0usize;
        let mut adopted = 0usize;
        let mut removed = 0usize;
        let our_device = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes());
        for e in entries {
            // Match FRIEND contacts only — sibling contacts carry OUR handle_proof and must not suppress a roster entry (nor be treated as its holder).
            if let Some(pos) = self
                .contacts
                .iter()
                .position(|c| !c.is_sibling && c.handle_proof == e.handle_proof)
            {
                if e.updated <= self.contacts[pos].roster_updated {
                    continue; // ours is as new or newer — our push carries it
                }
                if e.tombstone {
                    let gone = self.contacts.remove(pos);
                    crate::logf!(
                        "FLEET: roster tombstone — removed contact {}",
                        crate::fp(&gone.handle_proof).as_str()
                    );
                    // If the REMOVED contact's conversation (or panel) is open on THIS device, pop to the contact list. No index fixup exists any more: every OTHER open conversation is named by an id the remove cannot shift — the shift-by-one this replaces was the bug class where a missed fixup silently showed someone else's conversation.
                    let gone_id = self
                        .our_party_id(&gone)
                        .map(|us| gone.conversation(&us).id());
                    if gone_id.is_some() && self.active_conversation == gone_id {
                        self.broadcast_focus_claim(false);
                        self.active_conversation = None;
                        self.contact_boot_armed = false;
                        if matches!(
                            self.state,
                            AppState::Conversation | AppState::ContactPanel(_)
                        ) {
                            self.state = AppState::Ready;
                        }
                    }
                    if let Some(storage) = self.storage.as_ref() {
                        if let Err(err) =
                            crate::storage::contacts::delete_contact(&gone.handle_hash, storage)
                        {
                            crate::logf!("FLEET: tombstoned contact state delete failed: {}", err);
                        }
                        if let Some(fid) = gone.friendship_id {
                            self.friendship_chains.retain(|(id, _)| *id != fid);
                            if let Err(err) =
                                crate::storage::friendship::delete_friendship_chains(&fid, storage)
                            {
                                crate::logf!(
                                    "FLEET: tombstoned contact chain delete failed: {}",
                                    err
                                );
                            }
                        }
                    }
                    removed += 1;
                    continue;
                }
                let c = &mut self.contacts[pos];
                let pin_changed = c.avatar_pin != e.avatar_pin && e.avatar_pin != [0u8; 64];
                // The friend's chosen name rides the roster like the pin does (PRST4), so a fresh sibling shows real names without waiting for each friend's pong. Same empty-guard as the pin: a stub entry (re-attest re-push, empty fields, fresh LWW clock) means "never received", not "erase yours".
                if c.published_name != e.published_name && !e.published_name.is_empty() {
                    c.published_name = e.published_name.clone();
                }
                if pin_changed {
                    c.avatar_pin = e.avatar_pin;
                    c.avatar_pin_dirty = true; // post-drain sweep persists the index + refetches the avatar
                }
                // Trust rides the same last-writer-wins clock as the fields above (PRST3): a trust decision belongs to the identity, not to whichever device it was typed on. We are already past the `e.updated` gate, so a strictly-newer entry wins and ties keep ours.
                c.trust_level = crate::storage::cloud::u8_to_trust_level(e.trust_level);
                // §4.2 claim + the owner's completion, newest entry wins (same LWW as the fields above).
                let new_owner = (e.ceremony_owner != [0u8; 32]).then_some(e.ceremony_owner);
                // DISCARD-ON-PARK (fleet-sync.md §4.2: "a sibling DISCARDS its own parked offer toward the friend"): another device now owns this friendship's ceremony, so any round WE hold must die here — otherwise its keypairs/offer keep re-sending and the friend receives COMPETING ceremony instances (the "unknown conversation_token" stall). Also the takeover-LOSER path: a newer LWW claim only reaches this arm past the e.updated gate above. Never bumps roster_updated / never pushes — the winner's entry must stay newest.
                let owner_is_other = matches!(new_owner, Some(o) if Some(o) != our_device);
                let holds_round = c.clutch_our_keypairs.is_some()
                    || c.clutch_offer_sent
                    || !c.clutch_slots.is_empty()
                    || c.ceremony_id.is_some();
                if owner_is_other
                    && c.clutch_state != crate::types::ClutchState::Complete
                    && holds_round
                {
                    crate::logf!(
                        "CLUTCH §4.2: discarding parked round for {} — ceremony owner is {}",
                        crate::fp(&c.handle_proof).as_str(),
                        hex::encode(&e.ceremony_owner[..4])
                    );
                    c.discard_clutch_round();
                }
                c.ceremony_owner = new_owner;
                c.owner_woven = e.woven;
                c.roster_updated = e.updated;
                if let Some(storage) = self.storage.as_ref() {
                    if let Err(err) =
                        crate::storage::contacts::save_contact(&self.contacts[pos], storage)
                    {
                        crate::logf!("FLEET: roster-adopted contact save failed: {}", err);
                    }
                }
                adopted += 1;
                continue;
            }
            if e.tombstone {
                continue; // removal of a contact we never held — nothing to do locally
            }
            // SELF-GUARD: never mint a NEW row bearing OUR handle_proof under a foreign key — that is the self-contact stub (an empty duplicate whose keygen queue CLUTCHes at our own fleet; the census caught two of them beside the legit self row, 2026-08-11). The true notes-to-self row keys on our identity pid and merges thru the position() match above; anything else claiming our proof is stale roster debris.
            if let Some((our_proof, our_seed)) = self
                .session
                .as_ref()
                .map(|s| (s.handle_proof, s.identity_seed))
            {
                if e.handle_proof == our_proof
                    && e.handle_hash != crate::crypto::clutch::identity_party_id(&our_seed)
                {
                    crate::logf!("FLEET: roster entry claims OUR identity under a foreign key ({}…) — refused (self-stub debris)", hex::encode(&e.handle_hash[..4]));
                    continue;
                }
            }
            let device_pubkey = crate::types::DevicePubkey::from_bytes(e.public_identity);
            let mut contact = crate::types::Contact::from_pin(
                e.avatar_pin,
                e.handle_proof,
                e.handle_hash,
                device_pubkey,
            );
            contact.added = e.added;
            // The roster-carried published name lands on the stub immediately — this is the "avatars load instantly but names don't" fix for the re-attest path.
            contact.published_name = e.published_name.clone();
            // Carry the fleet's trust decision onto the stub. Defaulting here would silently DOWNGRADE a friend the user already promoted on another device — and a downgrade that arrives as "new contact" is exactly the case nothing would ever correct.
            contact.trust_level = crate::storage::cloud::u8_to_trust_level(e.trust_level);
            contact.roster_updated = e.updated;
            // §4.2: the entry names the fleet device running this friendship's ceremony — adopt the claim so THIS device parks instead of racing its own round at the friend. No discard hook needed here: a fresh from_pin contact holds no round state.
            contact.ceremony_owner = (e.ceremony_owner != [0u8; 32]).then_some(e.ceremony_owner);
            contact.owner_woven = e.woven;
            self.contacts.push(contact);
            added += 1;
        }
        if removed > 0 {
            // The index row must go with the contact, or the next launch resurrects it from the list.
            if let Some(storage) = self.storage.as_ref() {
                let index: Vec<crate::storage::contacts::ContactIdentity> = self
                    .contacts
                    .iter()
                    .filter(|c| !c.is_sibling)
                    .map(|c| crate::storage::contacts::ContactIdentity {
                        handle_proof: c.handle_proof,
                        party_id: c.handle_hash,
                        avatar_pin: c.avatar_pin,
                    })
                    .collect();
                if let Err(e) = crate::storage::contacts::save_contact_list(&index, storage) {
                    crate::logf!("FLEET: index rewrite after tombstone failed: {}", e);
                }
            }
            self.reseed_contact_pubkeys();
        }
        if adopted > 0 {
            crate::logf!(
                "FLEET: adopted {} newer roster entr(ies) from siblings",
                adopted
            );
        }
        if added == 0 {
            return;
        }
        crate::logf!(
            "FLEET: merged {} contact(s) from fleet roster (total: {})",
            added,
            self.contacts.len()
        );
        self.reseed_contact_pubkeys();
        // Re-fold every contact's fleet after a roster merge (newly-merged contacts have no members yet).
        let mut hps: Vec<[u8; 32]> = self
            .contacts
            .iter()
            .filter(|c| !c.is_sibling)
            .map(|c| c.handle_proof)
            .collect();
        hps.sort_unstable();
        hps.dedup();
        self.spawn_contact_fleet_refresh(hps);
        // Re-home any stale-keyed self row before persisting the newly-added tail. The keygen filter skips zero-remote conversations by itself — nothing to force.
        self.migrate_stale_self_row();
        let start = self.contacts.len() - added;
        if let Some(storage) = self.storage.as_ref().cloned() {
            for c in &self.contacts[start..] {
                if let Err(e) = crate::storage::contacts::save_contact(c, &storage) {
                    crate::logf!("FLEET: save merged contact failed: {}", e);
                }
            }
        }
        self.spawn_next_pending_keygen();
        // Freshly-merged contacts hold NO conversation rows yet — arm the fleet-history walk so a sibling backfills each one (fires once an online sibling exists).
        self.kick_fleet_history_sweep("roster merge");
    }

    /// Spawn a background pull of the fleet roster (debounced: one in flight at a time). The result is drained in `tick` and merged. No-op without a handle_proof + fleet key.
    pub(super) fn spawn_roster_pull(&mut self) {
        use crate::network::fgtw::fleet;
        if self.roster_pull_rx.is_some() {
            return;
        }
        let (Some(hp), Some(fleet_key)) = (self.our_handle_proof(), self.fleet_key_cached()) else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.roster_pull_rx = Some(rx);
        std::thread::spawn(move || {
            let result = match fleet::pull_fstate(&hp, &fleet_key) {
                Ok(Some(s)) => {
                    crate::logf!("FLEET: state pulled — {} roster entr(ies), {} global setting(s), {} device map(s)", s.roster.len(), s.global_settings.len(), s.device_settings.len());
                    Ok(s)
                }
                Ok(None) => {
                    crate::log("FLEET: state pull — slot empty");
                    Ok(fgtw::fstate::FleetState::default())
                }
                Err(e) => {
                    crate::logf!("FLEET: state pull failed: {}", e);
                    Err(e)
                }
            };
            let _ = tx.send(result);
        });
    }

    /// Lazily load the linked-settings cache (needs storage + our device pubkey). Returns whether it's available.
    pub(super) fn ensure_fleet_settings(&mut self) -> bool {
        if self.fleet_settings.is_some() {
            return true;
        }
        let (Some(storage), Some(kp)) = (self.storage.as_ref(), self.device_keypair.as_ref())
        else {
            return false;
        };
        self.fleet_settings = Some(crate::storage::fleet_settings::load_fleet_settings(
            storage,
            kp.public.to_bytes(),
        ));
        self.apply_settings_to_ui();
        self.publish_profile_name();
        self.publish_avatar_pin();
        true
    }

    /// Clear the AddDevice (existing-device) words-entry state. Sign the matched device into the fleet + rotate the fan-out, off-thread (both block on HTTP). Fires automatically when the typed words match the posted request — the deliberate act of typing 23 words on the already-trusted device IS the consent, and the new device's ready light has already flipped on the matched flag, so waiting for another tap only leaves the two screens out of step.
    /// Phase ONE of the two-phase ceremony: publish the consent-carrying Add for a fully-matched candidate. The chain entry is KEYLESS until the human confirms green — `spawn_confirm_add` holds the rotation.
    pub(super) fn spawn_bind_device(&mut self, req: crate::network::fgtw::fleet::BindRequest) {
        use crate::network::fgtw::fleet;
        self.add_device_status = "Words match \u{2014} adding\u{2026}".to_string();
        self.add_device_checking = true;
        if let (Some(hp), Some(kp), Some(tx)) = (
            self.our_handle_proof(),
            self.device_keypair.clone(),
            self.add_device_tx.clone(),
        ) {
            std::thread::spawn(move || {
                let _ = tx.send(match fleet::bind_device(&kp, &hp, &req) {
                    Ok(()) => AddDeviceUpdate::Bound(req.device_pubkey),
                    Err(e) => AddDeviceUpdate::Failed(e),
                });
            });
        }
    }

    /// Phase TWO — the human pressed "It's in" after seeing the new device leave its words screen: rotate the fleet key to the member set including the newcomer, releasing the key to it. Held behind the press so a wrong bind stays a keyless ledger entry.
    pub(super) fn spawn_confirm_add(&mut self) {
        use crate::network::fgtw::fleet;
        if self.add_device_checking || self.add_device_bound.is_none() {
            return;
        }
        self.add_device_status = "Finishing\u{2026}".to_string();
        self.add_device_checking = true;
        if let (Some(hp), Some(kp), Some(tx)) = (
            self.our_handle_proof(),
            self.device_keypair.clone(),
            self.add_device_tx.clone(),
        ) {
            let store = self.storage.clone();
            std::thread::spawn(move || {
                let r = fleet::current_members(&hp).and_then(|members| {
                    fleet::rotate_fleet_key(&hp, &kp, &members, store.as_deref()).map(|_| ())
                });
                let _ = tx.send(match r {
                    Ok(()) => AddDeviceUpdate::Rotated,
                    Err(e) => AddDeviceUpdate::Failed(e),
                });
            });
        }
    }

    /// The fleet device inventory for the Fleet settings page: this device first, then our other devices (sibling contacts). Each entry is `(device_pubkey, is_self, is_online, name)`. Reuses phase-1's sibling reconcile as the live inventory (`current_members(own_hp)` is the authority; reconcile keeps the sibling set == fleet-minus-this-device) — no synchronous network fetch on render. Name = the canonical `device_name_default` (two voca words from the device PUBLIC key + our identity seed), the SAME function the pairing screen uses, so a device shows the same name here, on its own pairing screen, and on every other device in THIS fleet (a handed-off device gets a fresh name in the new owner's fleet).
    /// Rows for the Fleet page: `(pubkey, is_self, online, retired, name, link)`. Current members first (self, then siblings), then the retired inventory — signed out, brand still ours (`fleet_retired`, refreshed on page entry).
    /// `link` is the sibling's LINK STATE in plain words — how far the secure channel to that device has got, plus whether its fan-out pair is egged (Phase A). It is the one place a stuck pairing is visible without reading a log: "securing 5/8" names whose side the ball is on, and "not egged yet" says the device cannot receive the fleet key until its ceremony finishes.
    #[allow(clippy::type_complexity)]
    pub(super) fn fleet_device_rows(
        &mut self,
    ) -> Vec<([u8; 32], bool, bool, bool, String, String, Option<u32>)> {
        use crate::network::fgtw::fleet::device_name_default;
        let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        let ours = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes());
        if let Some(me) = ours {
            // This device needs no tier — it IS the vantage point.
            rows.push((
                me,
                true,
                true,
                false,
                device_name_default(&me, &seed),
                String::new(),
                None,
            ));
        }
        // The egged probe remembered (the Fleet page gathers rows per FRAME; a vault read per sibling per frame is a librarian round trip for an answer that only changes at ceremony completion, which clears its entry).
        let mut egged_cache = std::mem::take(&mut self.egged_cache);
        for c in self.contacts.iter().filter(|c| c.is_sibling) {
            let pk = c.public_identity.key;
            // Egged = a completed CLUTCH minted this pair's fan-out secret; until then the device gets no wrap and holds no fleet key.
            let egged = *egged_cache.entry(pk).or_insert_with(|| {
                matches!((ours, self.storage.as_ref()), (Some(me), Some(st))
                    if crate::storage::fanout_pairs::load(&me, &pk, st).is_some())
            });
            let link = if !egged {
                format!("{} \u{00b7} not egged yet", c.clutch_status_detail())
            } else {
                c.clutch_status_detail()
            };
            rows.push((
                pk,
                false,
                c.is_online,
                false,
                device_name_default(&pk, &seed),
                link,
                // A sibling is another physical device — always a remote participant.
                path_tier_colour(c, true),
            ));
        }
        self.egged_cache = egged_cache;
        for pk in &self.fleet_retired {
            rows.push((
                *pk,
                false,
                false,
                true,
                device_name_default(pk, &seed),
                String::new(),
                None,
            ));
        }
        rows
    }

    /// Refresh the Fleet page's retired rows: chain history minus current members minus the `fleet.released` set (brands the owner already freed). Synchronous fetch, page-entry only — same style as the last-member gate. A fetch failure just hides the rows this visit (release is idempotent; nothing is lost).
    pub(super) fn refresh_fleet_retired(&mut self) {
        self.fleet_release_armed = None;
        self.fleet_lock_armed = None;
        self.fleet_unlock_armed = None;
        self.fleet_retired.clear();
        let Some(hp) = self.our_handle_proof() else {
            return;
        };
        match crate::network::fgtw::fleet::retired_devices(&hp) {
            Ok(list) => {
                let released = self.released_brands();
                self.fleet_retired = list
                    .into_iter()
                    .filter(|pk| !released.contains(pk))
                    .collect();
            }
            Err(e) => crate::logf!(
                "FLEET: retired fetch failed ({}) — rows hidden this visit",
                e
            ),
        }
    }

    /// The devices the fleet has LOCKED OUT (treat-as-stolen): per-key entries `fleet.locked.<hex pubkey>` unioned with the legacy one-blob key (see FleetSettings::pubkey_set_union for why per-key — the B4 race where concurrent locks of DIFFERENT devices dropped one). The sweep clears `locked_out` only on an AFFIRMATIVE emptied entry (see unlocked_tombstones), never on mere absence.
    pub(super) fn locked_devices(&self) -> Vec<[u8; 32]> {
        self.fleet_settings
            .as_ref()
            .map(|fs| fs.pubkey_set_union("fleet.locked", "fleet.locked."))
            .unwrap_or_default()
    }

    pub(super) fn is_locked_device(&self, pk: &[u8; 32]) -> bool {
        self.locked_devices().iter().any(|d| d == pk)
    }

    /// Sweep the fleet-synced locked set onto sibling contact rows (persisted, so refusal survives a relaunch before settings sync). Monotonic: locks only, never unlocks — an unlock is a deliberate future flow, not a merge artifact.
    pub(super) fn apply_locked_set(&mut self) {
        let mut locked = self.locked_devices();
        // SELF-LOCK: if OUR OWN device is in the fleet-locked set, this device has been locked out by the fleet (an honest owner locking a machine they no longer control).
        // Go dark — tear down the session so presence stops (it's state-gated to Ready/Conversation) and land on the launch screen.
        // Resume is handle-gated: the owner can re-attest, a thief without the handle cannot.
        // This is the self-side enforcement the fleet lockout lacked — it was one-directional (others refuse the device) with nothing making the device itself go quiet, so a locked machine kept broadcasting presence and rendering peers online.
        if self.session.is_some() {
            if let Some(ours) = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes()) {
                if locked.contains(&ours) {
                    crate::log("SELF-LOCK: this device is in the fleet-locked set — going dark (session cleared; re-type handle to unlock)");
                    tohu::clear_session();
                    self.session = None;
                    self.private_s = crate::crypto::blind::PrivateS::None;
                    self.pending_broadcast_signal = -1;
                    self.state = AppState::Launch(LaunchState::Fresh);
                    // The whole point of going dark is that resume is handle-gated — a field still holding the handle would gate nothing.
                    self.clear_handle_for_reproof();
                    return;
                }
            }
        }
        // The forgiveness side of the sweep: an EXPLICIT emptied entry (unlock_fleet_device here, or a sibling's synced tombstone) clears the row. Absence alone never clears — at boot the settings cache lags and the persisted rows below are the only truth, so only an affirmative tombstone may forgive.
        let unlocked = self.unlocked_tombstones();
        if !unlocked.is_empty() {
            let mut cleared: Vec<usize> = Vec::new();
            for (i, c) in self.contacts.iter_mut().enumerate() {
                if c.is_sibling && c.locked_out && unlocked.contains(c.public_identity.as_bytes()) {
                    c.locked_out = false;
                    cleared.push(i);
                }
            }
            if !cleared.is_empty() {
                if let Some(storage) = self.storage.as_ref() {
                    for &i in &cleared {
                        crate::logf!(
                            "FLEET: {} is UNLOCKED — the fleet trusts it again",
                            crate::fp(&self.contacts[i].public_identity.key)
                        );
                        let _ = crate::storage::contacts::save_contact(&self.contacts[i], storage);
                    }
                }
                self.reseed_contact_pubkeys();
            }
        }
        // Persisted rows count too: at boot the settings cache may not have loaded yet, but a row flagged in a previous session must keep its refusal from the first tick.
        for c in self
            .contacts
            .iter()
            .filter(|c| c.is_sibling && c.locked_out)
        {
            if !locked.contains(c.public_identity.as_bytes()) {
                locked.push(*c.public_identity.as_bytes());
            }
        }
        // The relay-layer refusal list rides every application of the set — including the empty one at boot, so a stale static from a prior identity can't linger.
        crate::network::fgtw::relay::set_locked_relay_targets(locked.clone());
        // And the pong tail's reported-stolen signal — the friend-side half rides the same edge.
        crate::network::status::set_locked_report(locked.clone());
        if locked.is_empty() {
            return;
        }
        let mut changed_rows: Vec<usize> = Vec::new();
        for (i, c) in self.contacts.iter_mut().enumerate() {
            if c.is_sibling && !c.locked_out && locked.contains(c.public_identity.as_bytes()) {
                c.locked_out = true;
                changed_rows.push(i);
            }
        }
        if !changed_rows.is_empty() {
            if let Some(storage) = self.storage.as_ref() {
                for &i in &changed_rows {
                    crate::logf!(
                        "FLEET: {} is LOCKED OUT — frames refused, key rotating away",
                        crate::fp(&self.contacts[i].public_identity.key)
                    );
                    let _ = crate::storage::contacts::save_contact(&self.contacts[i], storage);
                }
            }
            // Rebuild the answerable ping set so a freshly-locked device's PINGS stop being answered immediately (answerable_pubkeys now drops locked_out contacts) — not just its chain/ceremony frames, which the downstream knows_device gates already refused.
            // Without this the flat set kept the device until the next fold reseed, so it went on exchanging presence.
            self.reseed_contact_pubkeys();
        }
    }

    /// Treat-as-stolen: lock a fleet device out WITHOUT touching the membership chain (removal is self-signed only, zero exceptions). The device stays a permanent member; the fleet stops trusting it — the locked set syncs fleet-wide, every trust gate refuses it, and the fleet key rotates so its cached key goes stale.
    pub(super) fn lock_out_device(&mut self, pk: [u8; 32]) {
        // PER-KEY write, never read-union-write on one blob: two devices locking DIFFERENT pubkeys concurrently each unioned old+own into the same LWW key and one lock was DROPPED — a stolen device stayed trusted on part of the fleet until someone re-locked. Distinct keys commute under merge_global_settings, so a lock can no longer lose a race. The legacy blob stays readable (locked_devices unions it) and is never written again.
        if !self.is_locked_device(&pk) {
            self.settings_set(&format!("fleet.locked.{}", hex::encode(pk)), vsf::VsfType::ke(pk.to_vec()));
        }
        self.apply_locked_set();
        // Rotation NOW, not at the next sentinel pass: the locked device holds the current fleet key until an epoch it isn't wrapped into exists.
        self.spawn_fleet_key_sync();
        // Push the lock to the worker so the brick SURVIVES A WIPE: the local fleet.locked set above (and the fleet-key rotation) only bind devices that still hold local state, but a wiped-and-reattested stolen device lost all of that — the worker's device_lock entry is the one authority a wipe can't erase, refusing the device at announce.
        self.spawn_worker_lock_push(pk);
    }

    /// The owner's deliberate reversal of `lock_out_device` — fires only inside a handle-confirmed attest (see `pending_unlock`).
    /// Order is tombstone-first: the fleet-synced marker empties BEFORE the worker push, so every sibling's `reconcile_worker_locks` stops re-asserting the lock and `reconcile_worker_unlocks` re-drives the clear until the worker agrees — there is no strandable state in either direction.
    /// Re-admission is a GROW: the compliance rotation mints the next epoch including the freshly-eligible device; no shrink semantics involved.
    pub(super) fn unlock_fleet_device(&mut self, pk: [u8; 32], name: &str) {
        // Value-level tombstone, now honestly typed: u0(false) = "not locked". It never parses as a key on either read path (typed ke or legacy raw 32), so it drops out of pubkey_set_union on every build, and LWW carries the reversal fleet-wide.
        self.settings_set(&format!("fleet.locked.{}", hex::encode(pk)), vsf::VsfType::u0(false));
        // A pre-per-key lock may also live in the legacy one-blob key — rewrite it without this device (whole-blob LWW; the concurrent-writer race the per-key split fixed is tolerable on the rare unlock path).
        let legacy: Vec<u8> = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.effective("fleet.locked"))
            .and_then(|v| match v {
                vsf::VsfType::v(b'r', b) => Some(b.clone()),
                _ => None,
            })
            .unwrap_or_default();
        if legacy.chunks_exact(32).any(|c| c == pk) {
            let rewritten: Vec<u8> = legacy
                .chunks_exact(32)
                .filter(|c| *c != pk)
                .flatten()
                .copied()
                .collect();
            // The legacy blob keeps its legacy shape on rewrite — it is the one deliberately-raw survivor (deployed concatenated pubkeys, read forever, never re-typed).
            self.settings_set("fleet.locked", vsf::VsfType::v(b'r', rewritten));
        }
        self.apply_locked_set();
        self.spawn_worker_unlock_push(pk);
        self.spawn_fleet_key_rotate_for_compliance();
        self.ready_toast = Some(format!(
            "{name} unlocked \u{2014} it can rejoin the fleet on its next attest."
        ));
    }

    /// Best-effort off-thread push of ONE device unlock to the worker (fire-and-log). Idempotent (an absent lock is the goal state); the durable re-drive is `reconcile_worker_unlocks` on attest-success.
    pub(super) fn spawn_worker_unlock_push(&self, pk: [u8; 32]) {
        let hp = match self.our_handle_proof() {
            Some(hp) => hp,
            None => return,
        };
        let kp = match self.device_keypair.clone() {
            Some(kp) => kp,
            None => return,
        };
        std::thread::spawn(move || {
            match crate::network::fgtw::fleet::unlock_device(&kp, &hp, &pk) {
                Ok(()) => crate::logf!(
                    "FLEET: worker lock cleared for {} — the device can announce again",
                    crate::fp(&pk)
                ),
                Err(e) => crate::logf!(
                    "FLEET: worker unlock push failed for {} ({}) — will re-drive on next attest",
                    crate::fp(&pk),
                    e
                ),
            }
        });
    }

    /// Emptied per-key lock entries — devices the fleet has UNLOCKED (the value-level tombstones `unlock_fleet_device` writes). The pubkey rides the key's hex suffix since the value is deliberately empty.
    pub(super) fn unlocked_tombstones(&self) -> Vec<[u8; 32]> {
        let Some(fs) = self.fleet_settings.as_ref() else {
            return Vec::new();
        };
        fs.global
            .iter()
            .filter(|e| !e.tombstone && crate::storage::fleet_settings::as_key32(&e.value).is_none())
            .filter_map(|e| e.key.strip_prefix("fleet.locked."))
            .filter_map(|hexpk| {
                let bytes = hex::decode(hexpk).ok()?;
                <[u8; 32]>::try_from(bytes.as_slice()).ok()
            })
            .collect()
    }

    /// Re-push every locally-UNLOCKED device to the worker (durable re-drive, the inverse of `reconcile_worker_locks`): a failed unlock push — or a sibling's stale-lock re-push racing the settings sync — leaves the worker refusing a device the fleet forgave. Same attest-success cadence, idempotent puts.
    pub(super) fn reconcile_worker_unlocks(&self) {
        for pk in self.unlocked_tombstones() {
            self.spawn_worker_unlock_push(pk);
        }
    }

    /// Best-effort off-thread push of ONE device lock to the worker (fire-and-log). Idempotent at the worker (a plain put), so re-driving is harmless; the durable reconcile is `reconcile_worker_locks`, called on attest-success, which re-pushes every locally-locked device until the worker agrees.
    pub(super) fn spawn_worker_lock_push(&self, pk: [u8; 32]) {
        let hp = match self.our_handle_proof() {
            Some(hp) => hp,
            None => return,
        };
        let kp = match self.device_keypair.clone() {
            Some(kp) => kp,
            None => return,
        };
        std::thread::spawn(
            move || match crate::network::fgtw::fleet::lock_device(&kp, &hp, &pk) {
                Ok(()) => crate::logf!(
                    "FLEET: worker lock recorded for {} — brick now survives a wipe",
                    crate::fp(&pk)
                ),
                Err(e) => crate::logf!(
                    "FLEET: worker lock push failed for {} ({}) — will re-drive on next attest",
                    crate::fp(&pk),
                    e
                ),
            },
        );
    }

    /// Re-push every locally-locked device to the worker (durable reconcile). Called on attest-success — an infrequent, non-spammy edge — so a lock whose initial push failed (offline, stale-chain race) eventually reaches the worker; the puts are idempotent so re-pushing a known lock costs one no-op write.
    pub(super) fn reconcile_worker_locks(&self) {
        for pk in self.locked_devices() {
            self.spawn_worker_lock_push(pk);
        }
    }

    /// The brands the owner has released (hardware freed for a new identity): same per-key + legacy-blob union as the locked set — the release pill had the identical concurrent-write race, it just cost a reappearing row instead of a trust hole.
    pub(super) fn released_brands(&self) -> Vec<[u8; 32]> {
        self.fleet_settings
            .as_ref()
            .map(|fs| fs.pubkey_set_union("fleet.released", "fleet.released."))
            .unwrap_or_default()
    }

    /// Off-thread submit of this device's diagnostic log to FGTW (the Diagnostics "Submit" pill).
    /// The log can be up to 16 MiB and the POST blocks, so it runs on a thread and reports thru `log_submit_rx`. `note` is the user's optional-note textbox text. Snapshots the log bytes on the caller thread first (a plain file read) so a submit captures the log AT press time.
    /// Open (or re-sync) the Diagnostics log viewer: decode the whole on-disk log OFF-THREAD (16 MiB of per-line VSF records must not stall a frame) and land the rows via the channel tick drains. Also the rotation/Clear recovery path — a shrunken file re-syncs thru here from zero.
    pub(super) fn diag_log_open(&mut self) {
        self.diag_log_view = true;
        self.diag_log_follow = true;
        self.diag_log_rows = Vec::new();
        self.diag_log_consumed = 0;
        self.diag_log_next_poll_osc = 0;
        let (tx, rx) = std::sync::mpsc::channel();
        self.diag_log_rx = Some(rx);
        std::thread::spawn(move || {
            let bytes = crate::snapshot_log_bytes().unwrap_or_default();
            let (rows, consumed) = crate::parse_log_records(&bytes);
            let _ = tx.send((rows, consumed));
        });
    }

    /// Close the viewer and drop its rows (a capped log decodes to tens of MB of strings — not worth holding while the page shows the controls).
    pub(super) fn diag_log_close(&mut self) {
        self.diag_log_view = false;
        self.diag_log_rows = Vec::new();
        self.diag_log_rx = None;
        self.diag_log_inspect = None;
    }

    /// Log-viewer driver (tick): drain the off-thread initial decode, then tail-follow the live file — the size probe is one atomic load; the seek+decode runs at most twice a second and only for appended bytes. A shrink (16 MiB rotation or Clear) re-syncs from zero. Returns whether the view changed.
    pub(super) fn drive_diag_log(&mut self) -> bool {
        if !self.diag_log_view
            || !matches!(self.state, AppState::Settings(SettingsPage::Diagnostics))
        {
            return false;
        }
        let mut changed = false;
        if let Some(rx) = self.diag_log_rx.as_ref() {
            if let Ok((mut rows, consumed)) = rx.try_recv() {
                if rows.len() > DIAG_LOG_MAX_ROWS {
                    let excess = rows.len() - DIAG_LOG_MAX_ROWS;
                    rows.drain(..excess);
                }
                self.diag_log_rows = rows;
                self.diag_log_consumed = consumed as u64;
                self.diag_log_rx = None;
                changed = true;
            }
            return changed;
        }
        let now = vsf::eagle_time_oscillations();
        if now < self.diag_log_next_poll_osc {
            return false;
        }
        self.diag_log_next_poll_osc = now + (crate::OSC_PER_SEC >> 1);
        let size = crate::log_size_bytes();
        if size < self.diag_log_consumed {
            // Rotated or cleared — the offsets no longer mean anything; full re-sync.
            self.diag_log_open();
            return true;
        }
        if size > self.diag_log_consumed {
            if let Some(tail) = crate::read_log_from(self.diag_log_consumed) {
                let (mut rows, consumed) = crate::parse_log_records(&tail);
                if consumed > 0 {
                    self.diag_log_consumed += consumed as u64;
                    if !rows.is_empty() {
                        self.diag_log_rows.append(&mut rows);
                        if self.diag_log_rows.len() > DIAG_LOG_MAX_ROWS {
                            let excess = self.diag_log_rows.len() - DIAG_LOG_MAX_ROWS;
                            self.diag_log_rows.drain(..excess);
                        }
                        changed = true;
                    }
                }
            }
        }
        changed
    }

    pub(super) fn spawn_log_submit(&mut self, note: String) {
        use crate::network::fgtw::put_log_blocking;
        // Cheap emptiness probe only — the actual file read moves into the worker thread below: a long-lived log is hundreds of MB (a field device's never-yet-submitted buffer), and fs::read of that on the UI thread is its own hang.
        let total = crate::log_size_bytes();
        if total == 0 {
            self.ready_toast = Some("No log to send yet".to_string());
            return;
        }
        if self.log_submit_tx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            self.log_submit_rx = Some(rx);
            self.log_submit_tx = Some(tx);
        }
        // hp comes from the SESSION itself, not HandleQuery's worker-populated cache: a sticky-broadcast resume can reach Ready with `last_handle_proof` still unset, which made this gate claim "not signed in" to a user who plainly was (seen live, twice). Signed in ⇒ session ⇒ handle_proof; the cache is only a fallback.
        if let (Some(hp), Some(kp), Some(seed), Some(tx)) = (
            self.our_handle_proof(),
            self.device_keypair.clone(),
            self.session.as_ref().map(|s| s.identity_seed),
            self.log_submit_tx.clone(),
        ) {
            // Immediate press feedback — the read + upload run seconds on a big log, and silence here read as "the button did nothing". Replaced by "Log sent √" / "Send failed" when the worker thread reports.
            self.ready_toast = Some(format!(
                "Sending log ({} KiB)\u{2026}",
                (total as usize + 1023) / 1024
            ));
            std::thread::spawn(move || {
                let Some(bytes) = crate::snapshot_log_bytes() else {
                    let _ = tx.send(Err("log unreadable".to_string()));
                    return;
                };
                crate::logf!(
                    "DIAG: submitting log ({} bytes) to FGTW (sealed)",
                    bytes.len()
                );
                let r =
                    put_log_blocking(&bytes, &note, &kp, &hp, &seed).map_err(|e| format!("{e}"));
                let _ = tx.send(r);
            });
            self.log_submit_inflight = true;
        } else {
            self.ready_toast = Some("Can't send: not signed in".to_string());
        }
    }

    /// Rebuild the status checker's answerable-pubkey set from every contact's FULL fleet (`answerable_pubkeys` = first-met device union folded members). Idempotent — clears and refills, so it's safe after any change to contacts or their fleet_members. This is the single seam that makes presence + CLUTCH honour a friend's every device: seed here, and the offer/KEM/complete/SPEC gates (all of which read this one set) open for the whole fleet at once. The pong-seal key map reseeds from the same walk, so every call site — contacts load, roster merge, fold adoption, sibling reconcile — keeps both in lockstep for free.
    pub(super) fn reseed_contact_pubkeys(&self) {
        // OUR OWN device never enters the set: a contact row can carry us among its devices (the self row's first-met device is whichever of ours minted it; an echo-poisoned era added us as endpoints) — and a device that pings itself answers itself, which is the echo loop confirming its own poison (field, 2026-08-12).
        let ours = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes());
        if let Ok(mut pks) = self.contact_pubkeys.lock() {
            pks.clear();
            for c in &self.contacts {
                for k in c.answerable_pubkeys() {
                    if Some(k) == ours {
                        continue;
                    }
                    let dk = crate::types::DevicePubkey::from_bytes(k);
                    if !pks.contains(&dk) {
                        pks.push(dk);
                    }
                }
            }
        }
        self.reseed_pong_seal_keys();
    }

    /// Rebuild the pairwise pong-seal key map from the same contact walk as `reseed_contact_pubkeys` — one entry per answerable DEVICE pubkey, so the checker can seal a pong to whichever fleet device pinged. Friends: one key per friendship off the static identity DH ([`crate::crypto::clutch::identity_friendship_secret`]), inserted under each of their devices. Siblings — including a self-contact, whose party id IS our own — share the identity seed instead (their party ids aren't curve points, so the DH would come back None anyway) and key per sorted device pair. Runs on the UI thread only: the checker receives finished keys, never the seed.
    pub(super) fn reseed_pong_seal_keys(&self) {
        use zeroize::Zeroize;
        let Ok(mut keys) = self.pong_seal_keys.lock() else {
            return;
        };
        keys.clear();
        // No session (pre-attest / post-logout) or no device key = nothing derivable; the cleared map makes every pong go out tail-less, which is the safe direction.
        let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        let Some(our_device) = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes()) else {
            return;
        };
        let our_party = crate::crypto::clutch::identity_party_id(&seed);
        for c in &self.contacts {
            // A sibling row or a zero-remote conversation answers with OUR OWN devices, so those pongs seal under the sibling key; everyone else gets the friendship secret.
            if c.is_sibling || c.remote_count(&our_party) == 0 {
                for pk in c.answerable_pubkeys() {
                    // Spine present → the epoch-riding key (rotates per checkpoint, the B-arc pong re-seal); pre-spine → the v0 static derivation until bootstrap mints. ckpt_tick reseeds this map on every spine advance, so both ends flip keys on the same edge.
                    let key = match self.fleet_epoch.as_ref() {
                        Some((_, epoch)) => crate::network::status::sibling_pong_seal_key_epoch(
                            epoch,
                            &our_device,
                            &pk,
                        ),
                        None => crate::network::status::sibling_pong_seal_key(
                            &seed,
                            &our_device,
                            &pk,
                        ),
                    };
                    keys.insert(pk, key);
                }
            } else if let Some(mut fs) =
                crate::crypto::clutch::identity_friendship_secret(&seed, &c.handle_hash)
            {
                let key = crate::network::status::friend_pong_seal_key(&fs);
                // The DH output is live pairwise secret material — scrub the intermediate once the derived key exists.
                fs.zeroize();
                for pk in c.answerable_pubkeys() {
                    keys.insert(pk, key);
                }
            }
        }
    }

    pub(super) fn end_add_device_flow(&mut self) {
        // Stop the bindreq watch first — every exit path (back, orb, rotate-complete) kills the registry polling with the screen.
        if let Some(stop) = self.add_device_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.add_device_candidates.clear();
        self.add_device_bound = None;
        self.add_device_bind_ble = false;
        self.join_words_copied = false;
        self.add_device_wordcheck_text.clear();
        self.add_device_typo = None;
        self.add_device_checking = false;
        self.add_device_status.clear();
        self.add_device_rx = None;
        self.add_device_tx = None;
        if let Some(tb) = self.textbox.as_mut() {
            tb.clear();
        }
        // Drop focus off the words textbox — clearing chars doesn't unfocus it, so without this the (now hidden) widget keeps its blinkey firing, keystrokes still route to it, and Android's soft keyboard stays up over the contact list.
        self.change_focus(None);
    }

    /// New-device JOIN: the handle was entered — generate this device's pairing identity, display its words, post the signed request, and poll for the matched flag + membership off-thread.
    pub(super) fn submit_join_step(&mut self, precomputed_proof: Option<[u8; 32]>) {
        use crate::network::fgtw::fleet;
        use std::sync::atomic::{AtomicBool, Ordering};
        if self.add_join_rx.is_some() {
            return; // already joining
        }
        let handle: String = match self.textbox.as_ref() {
            Some(tb) => tb.chars.iter().collect(),
            None => return,
        };
        let handle = handle.trim().to_string();
        if handle.is_empty() {
            return;
        }
        let Some(device_key) = self.device_keypair.clone() else {
            self.add_join_status = "no device key".to_string();
            return;
        };
        // ONE IDENTITY PER DEVICE at the join door (docs/lifecycle.md D2): the direct join-mode entry (orb toggle → type handle) skips submit_handle's marker gate, and the worker's bindreq gate would only reject AFTER the words screen showed. Refuse a device bound to a different identity BEFORE any words, any beacon, any registry post.
        if let Some(bound) = crate::storage::device_binding::bound_party_id() {
            let typed_pid = crate::crypto::clutch::identity_party_id(
                &crate::types::Handle::to_identity_seed(&handle),
            );
            if typed_pid != bound {
                crate::log(
                    "join: DEVICE BUSY — bound to another identity; refusing before the words",
                );
                self.add_join_status = "this device already carries an identity \u{2014} put it on another device first, then Remove & shred (Settings \u{2192} Security)".to_string();
                return;
            }
        }
        self.add_join_handle = Some(handle.clone());
        self.add_join_status = "Preparing\u{2026}".to_string();
        self.change_focus(None);
        if let Some(tb) = self.textbox.as_mut() {
            tb.clear();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.add_join_rx = Some(rx);
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        self.add_stop = Some(stop.clone());
        std::thread::spawn(move || {
            // Derive the COMPLETE session roots once. identity_seed is microseconds; the ~1s memory-hard proof is reused from the probe when the caller passed it (the add-this-device branch already paid it), else computed here. Joined hands them to the attest worker so it never re-derives.
            let identity_seed = crate::storage::contacts::derive_identity_seed(&handle);
            let me = device_key.public.to_bytes();
            // SHOW THE WORDS FIRST — they only need identity_seed (microseconds) + the device pubkey, NOT the ~1s memory-hard handle_proof or the radio. Deferring this behind either left the screen on "Preparing…" for the whole proof (and, on Android, behind a blocking BLE-advertise JNI call) — the "stuck on Preparing" report. The words are this device's OWN pubkey masked to the fleet: shoulder-surfing them is inert (nothing binds without the request signature below; the mask makes them noise outside this fleet).
            let _ = tx.send(JoinUpdate::ShowWords(fleet::masked_device_words(
                &me,
                &identity_seed,
            )));
            // NOW the expensive derivation (reused from the probe when the caller passed it; else the ~1s proof here) — the words are already up, so this cost is invisible.
            let handle_proof = precomputed_proof
                .unwrap_or_else(|| crate::types::Handle::username_to_handle_proof(&handle));
            let session = tohu::SessionIdentity {
                identity_seed,
                vault_seed: identity_seed,
                handle_proof,
            };
            let hp = session.handle_proof;
            // NFC instant-add secret: 32 random bytes for THIS join session, served as a dumb tag over HCE (Android) while the ceremony is up. The put publishes its keyed commitment; a sponsor's tap reads S and matches it — proximity IS the selection. One S per session (the commitment re-binds each repost's fresh stamp).
            let nfc_secret: [u8; 32] = {
                use rand::RngCore;
                let mut s = [0u8; 32];
                rand::thread_rng().fill_bytes(&mut s);
                s
            };
            let _nfc_serve = crate::network::pairing_nfc::serve_guard(&nfc_secret);
            // The binding request: "I consent to join fleet hp", signed by the device key + co-signed by the identity key. THE registry entry the old device's matcher screens candidates from, and the consent egg the Add op will carry. Posted FIRST so its published eagle_time exists before we derive the proximity beacon from it.
            let offer_et = match fleet::bindreq_put(&device_key, &identity_seed, &hp, &nfc_secret) {
                Ok(et) => et,
                Err(e) => {
                    let _ = tx.send(JoinUpdate::Failed(format!("request failed: {e}")));
                    return;
                }
            };
            // Pairing v2 proximity beacon (docs/pairing-v2.md): advertise keyed_hash(device_pk ‖ published eagle_time) within the handle for the whole ceremony — the guard drops on every exit path below, stopping the radio. Re-announced on each repost (below) so the aired id tracks the offer's current stamp.
            let _beacon = crate::network::pairing_beacon::announce_guard(&hp, &me, offer_et);

            // Push subscription: the FGTW hub broadcasts `pair_evt` frames on registry changes ("request") and when the chain extends ("fleet"). Each one for OUR identity pokes `wake_tx`, and the poll loop's sleep below is a `recv_timeout` — so the ceremony reacts the moment the other device acts, with the poll cadence as the guarantee when the socket drops (best-effort accelerator, never load-bearing). The task dies with the socket or the stop flag; no reconnect — the poll still covers everything.
            let (wake_tx, wake_rx) = std::sync::mpsc::channel::<()>();
            {
                let stop = stop.clone();
                crate::network::http::runtime().spawn(async move {
                    use futures::StreamExt;
                    let Ok((mut ws, _)) = tokio_tungstenite::connect_async(crate::network::http::SEED_WS).await else {
                        return;
                    };
                    loop {
                        tokio::select! {
                            frame = ws.next() => {
                                match frame {
                                    Some(Ok(m)) => {
                                        if stop.load(Ordering::Relaxed) {
                                            return;
                                        }
                                        if let Some((_kind, evt_hp)) = fleet::parse_pair_event(&m.into_data()) {
                                            if evt_hp == hp {
                                                let _ = wake_tx.send(());
                                            }
                                        }
                                    }
                                    _ => return, // closed or errored — poll cadence takes over
                                }
                            }
                            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                                if stop.load(Ordering::Relaxed) {
                                    return;
                                }
                            }
                        }
                    }
                });
            }
            // PUSH-DRIVEN, no deadline, no poll cadence. The hub events (request / fleet) wake each check instantly; the ONLY timers are the ones the protocol/transport demand — the binding request's 5-minute freshness (re-post at ~3.5min when no event arrives sooner) and a degraded-transport fallback cadence when the socket is dead. The user standing at the screen is the timeout: the ceremony ends when the bind lands, when they tap the orb (the stop flag), or on a hard network error. The request is WITHDRAWN by this device (its author) on every exit — green, cancel, or wrong-fleet — and lapses by stamp if the thread dies unclean.
            let withdraw = |why: &str| {
                if let Err(e) = fleet::bindreq_withdraw(&device_key, &hp) {
                    crate::logf!(
                        "JOIN: request withdraw ({}) failed (lapses anyway): {}",
                        why,
                        e
                    );
                }
            };
            let mut last_repost = std::time::Instant::now();
            let ceremony_start = std::time::Instant::now();
            // Give-up backstop: a real ceremony binds in seconds-to-a-minute. Past this it's a forgotten or backgrounded screen — and Android keeps this raw thread alive after the Activity pauses (onPause emits no focus event), so without a cap it re-posts the registry every 3.5 min forever (observed: a backgrounded phone re-posting for 1.6h). Generous enough that a live pairing never hits it.
            const MAX_CEREMONY: std::time::Duration = std::time::Duration::from_secs(15 * 60);
            loop {
                if stop.load(Ordering::Relaxed) {
                    withdraw("cancel");
                    return;
                }
                if ceremony_start.elapsed() >= MAX_CEREMONY {
                    crate::log("JOIN: ceremony ran too long without binding — stopping re-posts");
                    withdraw("timeout");
                    let _ = tx.send(JoinUpdate::Failed(
                        "timed out waiting to be added — re-enter the handle to try again".into(),
                    ));
                    return;
                }
                // Genesis re-checked on EVERY fetch, not just the probe: a relay that served the real chain once must not be able to swap in a structurally-valid foreign chain mid-ceremony and have this loop adopt its members (the probe-time-only TOCTOU — docs/pairing-v2.md).
                match fleet::current_members_verified(&hp) {
                    Ok(m) if m.contains(&me) => {
                        // In the fleet — this is the green, and LEAVING THIS SCREEN is what the far-side human confirms, so it must happen NOW, not after the key. Withdraw our request (the author's exit act), try the fan-out once in case the sponsor already confirmed, and hand off — the key otherwise arrives via the post-attest fleet-event sync the moment the green-confirm rotation lands (the worker broadcasts "fleet" on fanout_put). Waiting here deadlocks the ceremony: the sponsor waits for our green before releasing the key this wait was for.
                        crate::log("JOIN: bound — this device is in the fleet chain");
                        withdraw("green");
                        // No storage yet (the vault opens at attest), so no pair secret toward the sponsor — this attempt can only succeed once we are egged. The key arrives via the post-attest sync after our sibling CLUTCH with the sponsor mints the pair secret and the sponsor re-rotates (Phase A compliance).
                        let fleet_key = fleet::recover_fleet_key(&hp, &device_key, None)
                            .ok()
                            .flatten();
                        crate::logf!(
                            "JOIN: fleet key {} — attesting",
                            if fleet_key.is_some() {
                                "recovered from fan-out"
                            } else {
                                "follows the sponsor's confirm (event-synced)"
                            }
                        );
                        let _ = tx.send(JoinUpdate::Joined(fleet_key, session));
                        return;
                    }
                    Ok(_) => {} // not bound yet — keep the request fresh and wait
                    Err(e) if e.contains("not rooted in this identity") => {
                        // Wrong-fleet is a VERDICT, not a blip: the chain now folds to a genesis that isn't ours, mid-ceremony. Scream and stop — retrying would just poll an imposter chain forever.
                        crate::log(
                            "JOIN: chain genesis no longer matches this identity — aborting",
                        );
                        withdraw("wrong-fleet");
                        let _ = tx.send(JoinUpdate::Failed(
                            "this handle's fleet is not ours — the chain changed hands mid-join"
                                .into(),
                        ));
                        return;
                    }
                    Err(e) => {
                        // A transient fetch failure (laptop sleep/wake, dropped wifi, a server blip) must NOT kill the ceremony — the user is still standing at the words screen, and a dead thread strands it there forever (observed: macbook stuck on its words after one failed fetch). Log, back off, retry; the stop flag (orb cancel) is the only exit.
                        crate::logf!("JOIN: membership fetch failed ({}) — retrying", e);
                        std::thread::sleep(crate::jitter_dur(std::time::Duration::from_secs(15)));
                        continue;
                    }
                }
                // Wait for a push, but re-poll membership on a SHORT cadence so the ceremony completes fast even when the hub WebSocket push never arrives (observed on macOS: the bind landed but the new device sat ~3.5 min on the old 210s timeout before its next `current_members` check). The request re-post stays on its protocol cadence (~3.5 min — the registry stamp lapses at 5), tracked separately so the frequent membership polls don't spam re-posts. A hub event still short-circuits instantly.
                const MEMBERSHIP_POLL: std::time::Duration = std::time::Duration::from_secs(8);
                const REPOST_EVERY: std::time::Duration = std::time::Duration::from_secs(210);
                match wake_rx.recv_timeout(crate::jitter_dur(MEMBERSHIP_POLL)) {
                    Ok(()) => {} // hub push — loop re-checks membership immediately
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if last_repost.elapsed() >= REPOST_EVERY {
                            // Restamp the offer, then re-emit the beacon under the NEW eagle_time so the aired id keeps matching the list entry the sponsor now reads.
                            if let Ok(et) =
                                fleet::bindreq_put(&device_key, &identity_seed, &hp, &nfc_secret)
                            {
                                crate::network::pairing_beacon::reannounce(&hp, &me, et);
                            }
                            last_repost = std::time::Instant::now();
                        }
                        // else: just loop and re-poll current_members (the fast path)
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        std::thread::sleep(crate::jitter_dur(std::time::Duration::from_secs(8)));
                        if last_repost.elapsed() >= REPOST_EVERY {
                            // Restamp the offer, then re-emit the beacon under the NEW eagle_time so the aired id keeps matching the list entry the sponsor now reads.
                            if let Ok(et) =
                                fleet::bindreq_put(&device_key, &identity_seed, &hp, &nfc_secret)
                            {
                                crate::network::pairing_beacon::reannounce(&hp, &me, et);
                            }
                            last_repost = std::time::Instant::now();
                        }
                    }
                }
            }
        });
    }
}
