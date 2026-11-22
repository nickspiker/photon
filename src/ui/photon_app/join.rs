//! The new device's join: the Join machine of fgtw::flow (FLOWS.md stage 4, the owner's rules of 2026-11-22) carried out with Photon's radios, sockets and screen.
//! The look-up's verdict makes the machine; the KnownHandle screen asks its question; only the person's yes lets anything leave this device, and from then on the machine says what goes where: the LAN beacon, the request handed to each fleet device heard, NFC, BLE, and fgtw.org's registry only on the last-rung edge or "Not in range?".
//! The watch thread holds the identity roots and turns the joiner green from a chain or a pick handed over the mesh (and fgtw.org's copy of the chain once the registry holds the request).

use super::*;
use fgtw::flow::join::{Do, JoinNote, Phase};
use fgtw::pq::FleetSigner;

impl PhotonApp {
    /// The look-up found the identity's fleet (the probe's JoinOurs, or an attest the fleet refused as "not in the fleet"): make the Join machine with the roots the proof already paid for, and tell it what was found.
    /// The device-busy gate is the machine's input here, computed after the memory-hard proof so the binding marker is never a cheap handle oracle (2026-08-23 ticket).
    pub(super) fn join_look(&mut self, session: tohu::SessionIdentity) {
        self.end_join_flow();
        let Some(me) = self.device_keypair.as_ref().map(|kp| kp.public.to_bytes()) else {
            self.state = AppState::Launch(LaunchState::Error(tr(fgtw::text::Msg::NoDeviceKey).into_owned()));
            return;
        };
        let busy = crate::storage::device_binding::busy_for(&session.handle_proof, &crate::crypto::clutch::identity_party_id(&session.identity_seed));
        self.join_roots = Some(session);
        self.join = Some(fgtw::flow::join::Join::new(me, session.identity_seed));
        self.join_step(fgtw::flow::Event::Looked { has_fleet: true, member: false, busy });
    }

    /// The person's yes ("Yes, I really would like to add this device and this is my identity and not a typo"): the one press that lets the join out.
    pub(super) fn join_confirm(&mut self) {
        if !self.join.as_ref().is_some_and(|j| j.phase() == Phase::Confirm) {
            return;
        }
        let Some(roots) = self.join_roots else { return };
        // The tap secret for this join: 32 random bytes the app draws (a machine draws none), served over NFC while the machine holds it on.
        let secret: [u8; 32] = {
            use rand::RngCore;
            let mut s = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut s);
            s
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.add_join_rx = Some(rx);
        self.join_tx = Some(tx);
        self.join_nfc_secret = Some(secret);
        self.add_join_hp = Some(roots.handle_proof);
        self.add_join_sponsors.clear();
        self.join_selected_by = None;
        self.join_words_copied = false;
        self.launch_add_mode = true;
        self.state = AppState::Launch(LaunchState::Fresh);
        self.change_focus(None);
        crate::log("JOIN: the person confirmed on this device; the request goes to their devices nearby");
        self.join_step(fgtw::flow::Event::User(fgtw::flow::UserOp::Confirm { nfc_secret: secret }));
    }

    /// One event to the Join machine: its view kept for the screen, its deadline for the driver, its acts carried out.
    pub(super) fn join_step(&mut self, ev: fgtw::flow::Event) {
        let Some(m) = self.join.as_mut() else { return };
        let step = m.step(ev, vsf::eagle_time_oscillations());
        let view = m.view();
        // The words exist on screen only when the machine shows them ("Not in range?").
        self.add_join_words = view.words.clone();
        self.join_view = Some(view);
        self.join_wake = step.wake;
        self.join_carry(step.actions);
    }

    /// The join is asking (past the yes): the join screen is up.
    pub(super) fn join_asking(&self) -> bool {
        self.launch_add_mode && self.join_view.as_ref().is_some_and(|v| v.phase == Phase::Asking)
    }

    /// Per tick: the Join machine's one deadline (the hearing window, or the request's re-mint). Returns whether it stepped.
    pub(super) fn join_tick(&mut self) -> bool {
        let Some((at, reason)) = self.join_wake else { return false };
        if vsf::eagle_time_oscillations() < at {
            return false;
        }
        self.join_wake = None;
        self.join_step(fgtw::flow::Event::Wake { reason });
        true
    }

    /// The Join machine's deadline as an Instant for the driver's wake; one already past is due now.
    pub(super) fn join_wake_at(&self, now: std::time::Instant) -> Option<std::time::Instant> {
        let (at, _) = self.join_wake?;
        let ahead = at - vsf::eagle_time_oscillations();
        // WHY: Duration has no negative values (from_secs_f64 panics on one), and a deadline the clock already passed is simply due.
        Some(if ahead > 0 { now + std::time::Duration::from_secs_f64(ahead as f64 / vsf::OSCILLATIONS_PER_SECOND as f64) } else { now })
    }

    /// A fleet device of the identity this device is joining was heard on this network: remember where, and tell the machine (it hands the request once per stamp).
    pub(super) fn join_heard(&mut self, device: [u8; 32], addr: std::net::SocketAddr) {
        match self.add_join_sponsors.iter_mut().find(|(k, _)| *k == device) {
            Some(e) => e.1 = addr,
            None => {
                crate::logf!("JOIN: a device of the fleet heard at {}", addr);
                self.add_join_sponsors.push((device, addr));
            }
        }
        self.join_step(fgtw::flow::Event::LanHeard { device });
    }

    fn join_carry(&mut self, acts: Vec<Do>) {
        for d in acts {
            match d {
                Do::Mint => self.join_mint(),
                Do::Hand { to, frame } => match self.add_join_sponsors.iter().find(|(k, _)| *k == to).map(|(_, a)| *a) {
                    Some(addr) => {
                        crate::logf!("JOIN: handing our binding request to {} at {}", crate::fp(&to), addr);
                        self.send_frame_direct(frame, to, addr, None);
                    }
                    None => crate::logf!("JOIN: no address for {}; the request waits for its beacon", crate::fp(&to)),
                },
                Do::LanBeacon => {
                    let (Some(roots), Some(checker)) = (self.join_roots, self.status_checker.as_ref()) else { continue };
                    let port = self.handle_query.as_ref().map(|h| h.port()).unwrap_or(crate::PHOTON_PORT);
                    // One beacon under the handle: every fleet device on this network answers it straight back (the LanPeerDiscovered arm), and each answer is the machine's LanHeard.
                    checker.send_lan_broadcast(roots.handle_proof, port);
                }
                Do::Ble(Some(t)) => {
                    let (Some(roots), Some(me)) = (self.join_roots, self.device_keypair.as_ref().map(|kp| kp.public.to_bytes())) else { continue };
                    // The radio calls are the ones the sponsor's scan makes from this thread too; the guard stops the advert on every exit.
                    match self.join_beacon {
                        Some(_) => crate::network::pairing_beacon::reannounce(&roots.handle_proof, &me, t),
                        None => self.join_beacon = Some(crate::network::pairing_beacon::announce_guard(&roots.handle_proof, &me, t)),
                    }
                }
                Do::Ble(None) => self.join_beacon = None,
                Do::Nfc(s) => self.join_nfc = s.map(|s| crate::network::pairing_nfc::serve_guard(&s)),
                Do::PostRegistry => self.join_post(),
                Do::Withdraw => {
                    let (Some(roots), Some(kp)) = (self.join_roots, self.device_keypair.clone()) else { continue };
                    std::thread::spawn(move || {
                        if let Err(e) = crate::network::fgtw::fleet::bindreq_withdraw(&kp, &roots.handle_proof) {
                            crate::logf!("JOIN: the request's withdraw from fgtw.org failed (it lapses anyway): {}", e);
                        }
                    });
                }
                Do::Watch { registry } => {
                    if self.add_stop.is_none() {
                        self.join_watch_start();
                    }
                    if registry {
                        if let Some(flag) = self.join_mailbox.as_ref() {
                            flag.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                        if let Some(poke) = self.add_join_poke.as_ref() {
                            crate::log_err!(poke.send(()), "the registry poke to the join watch");
                        }
                    }
                }
                Do::Unwatch => {
                    if let Some(stop) = self.add_stop.take() {
                        stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    if let Some(poke) = self.add_join_poke.take() {
                        crate::log_err!(poke.send(()), "the stop poke to the join watch");
                    }
                }
                Do::Note(n) => self.join_note(n),
            }
        }
    }

    /// Do::Mint: the binding request minted and signed (all eggs: off this thread), with the tap secret drawn at the yes.
    fn join_mint(&self) {
        let (Some(roots), Some(secret), Some(tx), Some(device_key)) = (self.join_roots, self.join_nfc_secret, self.join_tx.clone(), self.device_signer()) else {
            crate::log("JOIN: nothing to mint the request with (no roots, secret or device key)");
            return;
        };
        let wake = self.event_proxy.clone();
        std::thread::spawn(move || {
            let req = crate::network::fgtw::fleet::make_bind_request(&device_key, &roots.identity_seed, &roots.handle_proof, &secret);
            let r = crate::network::fgtw::protocol::build_bind_req_vsf(&req, &roots.handle_proof, device_key.keypair().secret.as_bytes()).map(|frame| (req, frame));
            crate::log_err!(tx.send(JoinUpdate::Minted(r)), "the minted request to the UI thread");
            if let Some(w) = wake.as_ref() {
                super::wake_ui(w);
            }
        });
    }

    /// Do::PostRegistry: the request as last minted to fgtw.org's registry, the last rung.
    fn join_post(&self) {
        let (Some(roots), Some(req), Some(tx)) = (self.join_roots, self.join_req.clone(), self.join_tx.clone()) else { return };
        let wake = self.event_proxy.clone();
        std::thread::spawn(move || {
            let r = crate::network::fgtw::fleet::bindreq_post(&req, &roots.handle_proof);
            match &r {
                Ok(()) => crate::log("JOIN: the binding request is in fgtw.org's registry"),
                Err(e) => crate::logf!("JOIN: fgtw.org did not take the binding request: {}", e),
            }
            crate::log_err!(tx.send(JoinUpdate::Posted(r)), "the registry answer to the UI thread");
            if let Some(w) = wake.as_ref() {
                super::wake_ui(w);
            }
        });
    }

    /// What the Join machine says the person should be told.
    fn join_note(&mut self, n: JoinNote) {
        match n {
            JoinNote::Confirm => {
                // The question, nothing sent: the KnownHandle screen carries the owner's sentence as its button.
                crate::log("attest: handle already has a fleet; asking before anything goes out");
                self.state = AppState::Launch(LaunchState::KnownHandle);
            }
            JoinNote::Busy => {
                crate::log("JOIN: DEVICE BUSY: bound to another identity; refused post-proof");
                self.end_join_flow();
                self.state = AppState::Launch(LaunchState::Error(tr(Msg::IdentityOccupied).into_owned()));
                self.refocus_handle_select_all();
            }
            JoinNote::Asking => self.add_join_status = tr(fgtw::text::Msg::AddFromSignedIn).into_owned(),
            JoinNote::Unheard => crate::log("JOIN: no device of the fleet heard on this network; the request goes to fgtw.org, the last rung"),
            JoinNote::Owned => {
                crate::log("JOIN: fgtw.org says this device belongs to another identity's fleet");
                self.add_join_status = tr(fgtw::text::Msg::OwnedByOther).into_owned();
            }
            JoinNote::PostFailed(e) => crate::logf!("JOIN: fgtw.org did not take the request ({}); the devices nearby still have it", e),
            JoinNote::MintFailed(e) => {
                crate::logf!("JOIN: the binding request could not be minted: {}", e);
                self.end_join_flow();
                self.launch_add_mode = false;
                self.state = AppState::Launch(LaunchState::Error(tr(Msg::JoinFailed(&e)).into_owned()));
                self.refocus_handle_select_all();
            }
            JoinNote::Selected { by } => crate::logf!("JOIN: selected by {}; green, the keys follow the Add there", crate::fp(&by)),
            JoinNote::Joined => crate::log("JOIN: in the fleet; radios off"),
            JoinNote::Withdrawn => crate::log("JOIN: withdrawn; the request is dropped and the radios are off"),
        }
    }

    /// Do::Watch: the thread that turns this joiner green. It holds the identity roots and checks every chain and pick handed to this device over the mesh against them; once fgtw.org holds the request it also reads fgtw.org's copy of the chain (the last rung).
    fn join_watch_start(&mut self) {
        use crate::network::fgtw::fleet;
        use std::sync::atomic::{AtomicBool, Ordering};
        let (Some(session), Some(kp), Some(tx)) = (self.join_roots, self.device_keypair.clone(), self.join_tx.clone()) else { return };
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        self.add_stop = Some(stop.clone());
        let registry = std::sync::Arc::new(AtomicBool::new(false));
        self.join_mailbox = Some(registry.clone());
        let (poke_tx, wake_rx) = std::sync::mpsc::channel::<()>();
        self.add_join_poke = Some(poke_tx.clone());
        let chain_slot = self.add_join_chain.clone();
        let fanout_slot = self.add_join_fanout.clone();
        let select_slot = self.add_join_select.clone();
        for s in [&chain_slot, &fanout_slot] {
            if let Ok(mut g) = s.lock() {
                *g = None;
            }
        }
        if let Ok(mut g) = select_slot.lock() {
            *g = None;
        }
        let wake_ui = self.event_proxy.clone();
        std::thread::spawn(move || {
            let hp = session.handle_proof;
            let identity_seed = session.identity_seed;
            let me = kp.public.to_bytes();
            let send = |u: JoinUpdate| {
                crate::log_err!(tx.send(u), "a join update to the UI thread");
                if let Some(w) = wake_ui.as_ref() {
                    super::wake_ui(w);
                }
            };
            // GREEN: this device folds into the fleet's chain. A fan-out handed over the mesh opens at once; else fgtw.org's; else the post-attest sync.
            let green = |how: &str| {
                crate::logf!("JOIN: bound: this device is in the fleet chain ({})", how);
                let pushed = fanout_slot.lock().ok().and_then(|mut s| s.take()).and_then(|blob| fleet::fanout_open_blob(&hp, &blob, &kp, &identity_seed).ok().flatten().map(|(_, k)| k));
                let fleet_key = pushed.or_else(|| fleet::recover_fleet_key(&hp, &kp, &identity_seed).ok().flatten());
                send(JoinUpdate::Joined(fleet_key, session));
            };
            let mut known_chain: Option<fleet::MembershipBlob> = None;
            let mut pending_select: Option<[u8; 32]> = None;
            let mut hub_on = false;
            loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let at_registry = registry.load(Ordering::Relaxed);
                // fgtw.org's hub pokes this watch when the chain extends, once the registry holds the request (best-effort accelerator, never load-bearing).
                #[cfg(not(target_os = "ferros"))]
                if at_registry && !hub_on {
                    hub_on = true;
                    let (stop, poke) = (stop.clone(), poke_tx.clone());
                    crate::network::http::runtime().spawn(async move {
                        use futures::StreamExt;
                        let Ok((mut ws, _)) = tokio_tungstenite::connect_async(crate::network::http::SEED_WS).await else { return };
                        while let Some(Ok(m)) = ws.next().await {
                            if stop.load(Ordering::Relaxed) {
                                return;
                            }
                            if fleet::parse_pair_event(&m.into_data()).is_some_and(|(_, evt_hp)| evt_hp == hp) && poke.send(()).is_err() {
                                return;
                            }
                        }
                    });
                }
                #[cfg(target_os = "ferros")]
                let _ = &mut hub_on;
                // THE MESH FIRST: a chain a sponsor pushed to us, verified exactly as a seed copy is (genesis this identity's, it folds); green iff it folds us in, else kept to check a pick against.
                if let Some(bytes) = chain_slot.lock().ok().and_then(|mut s| s.take()) {
                    match fleet::MembershipBlob::from_vsf_bytes(&bytes) {
                        Ok(blob) if blob.genesis_handle_proof() == Some(hp) && blob.genesis_identity_matches(&identity_seed) => match blob.fold() {
                            Ok(members) => {
                                fleet::hold_chain(&blob);
                                if members.contains(&me) {
                                    green("over the mesh");
                                    return;
                                }
                                crate::log("JOIN: a chain for our fleet arrived over the mesh; not bound yet");
                                known_chain = Some(blob);
                            }
                            Err(e) => crate::logf!("SECURITY: a chain pushed to this joiner does not fold ({}); ignored", format!("{:?}", e)),
                        },
                        Ok(_) => crate::log("SECURITY: a chain pushed to this joiner is not this identity's fleet; ignored"),
                        Err(e) => crate::logf!("JOIN: a chain pushed to this joiner did not parse ({}); ignored", e),
                    }
                }
                // A sponsor's pick (bind_select): green only when the sender folds as a live member of this identity's chain; held until a chain arrives to check it against.
                if let Some(s) = select_slot.lock().ok().and_then(|mut g| g.take()) {
                    pending_select = Some(s);
                }
                if let Some(sender) = pending_select {
                    let chain = known_chain.clone().or_else(|| fleet::held_chain(&hp).filter(|c| c.genesis_handle_proof() == Some(hp) && c.genesis_identity_matches(&identity_seed)));
                    if let Some(c) = chain {
                        pending_select = None;
                        let live = c.fold().ok().zip(c.locked_out().ok()).is_some_and(|(m, l)| m.contains(&sender) && !l.contains(&sender));
                        if live {
                            send(JoinUpdate::Selected { by: sender, name: fleet::device_name_default(&sender, &identity_seed) });
                        } else {
                            crate::logf!("SECURITY: a pick from {} is not from a live member of this fleet; ignored", crate::fp(&sender));
                        }
                    }
                }
                // fgtw.org's copy of the chain, only once it holds the request (the last rung). Genesis is checked on every read, so a relay cannot swap in a foreign chain mid-join.
                if at_registry {
                    match fleet::current_members_verified(&hp) {
                        Ok(m) if m.contains(&me) => {
                            green("by fgtw.org's copy");
                            return;
                        }
                        Ok(_) => {}
                        Err(e) if e.contains("not rooted in this identity") => {
                            crate::log("JOIN: the chain's genesis no longer matches this identity; aborting");
                            send(JoinUpdate::Failed("this handle's fleet is not ours\nthe chain changed hands mid-join".into()));
                            return;
                        }
                        Err(e) => crate::logf!("JOIN: fgtw.org's chain read failed ({}); the mesh may answer first", e),
                    }
                }
                // Nearby, every answer is a push (a chain, a pick, a fan-out): wait for one, no clock.
                // With fgtw.org holding the request its chain is read again every 8 s as well: the hub's push is an accelerator that may never come, and that read is the fallback rung's own guarantee.
                let woke = if at_registry {
                    match wake_rx.recv_timeout(crate::jitter_dur(std::time::Duration::from_secs(1 << 3))) {
                        Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => true,
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => false,
                    }
                } else {
                    wake_rx.recv().is_ok()
                };
                if !woke {
                    return;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    /// Photon's join sources, read as text: the paths that used to send with no press are checked by where the calls are, since they were call sites (owner, 2026-11-22).
    fn sources() -> [&'static str; 10] {
        [
            include_str!("launch.rs"),
            include_str!("driver.rs"),
            include_str!("protocol.rs"),
            include_str!("status.rs"),
            include_str!("devices.rs"),
            include_str!("input.rs"),
            include_str!("join.rs"),
            include_str!("render.rs"),
            include_str!("settings.rs"),
            include_str!("../photon_app.rs"),
        ]
    }

    fn count(needle: &str) -> usize {
        sources().iter().map(|f| f.matches(needle).count()).sum()
    }

    /// A join goes out only on the person's yes: the Join machine refuses everything before UserOp::Confirm (fgtw's join_tests), Photon's one Confirm is join_confirm, and its one caller is the KnownHandle screen's yes pill.
    /// The "not in the fleet" safety net and a handle typed again in join mode reach the question (join_look), never a send; the old thread that sent at once is gone.
    #[test]
    fn the_only_way_out_is_the_yes_pill() {
        // Needles built from pieces, so this test's own text is not counted.
        assert_eq!(count(&["UserOp::", "Confirm {"].concat()), 1, "one Confirm, in join_confirm");
        assert_eq!(count(&["self.", "join_confirm()"].concat()), 1, "one caller: the yes pill");
        assert!(sources()[1].contains(&["self.", "join_confirm()"].concat()), "the caller is the driver's KnownHandle press");
        assert_eq!(count(&["submit_", "join_step"].concat()), 0, "the join thread that sent with no press is gone");
        let launch = sources()[0];
        assert!(!launch.contains(&["join_", "confirm"].concat()), "launch.rs (the safety net, the handle typed again) never confirms");
        assert!(launch.contains(&["self.", "join_look(session)"].concat()), "the safety net and the probe reach the question");
        // fgtw.org's registry, the BLE beacon and the tap secret are each reached from one place, the Join machine's acts (the Depart machine serves its own salt).
        assert_eq!(count(&["fleet::", "bindreq_post("].concat()), 1);
        assert_eq!(count(&["pairing_beacon::", "announce_guard("].concat()), 1);
        assert_eq!(count(&["pairing_nfc::", "serve_guard("].concat()), 2);
    }
}
