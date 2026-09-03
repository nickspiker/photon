//! `advance_protocol` — the surface-free network pump: presence sweeps, channel drains, and CLUTCH + chain advancement, driven by `tick` on desktop and headlessly by the Android foreground service.

use super::*;

impl PhotonApp {
    /// The surface-free half of `tick`: presence pinging, draining every network/background channel, and advancing the CLUTCH ceremony + message chains. Returns `true` if anything changed (the caller turns that into a redraw request). Split out of `tick` so the Android foreground service can drive it headlessly while backgrounded — the paused Activity's Choreographer has stopped calling `tick`, but `PhotonApp` is alive and its inbound CLUTCH/chat still needs to advance so ceremonies complete and messages get ACKed without the screen being on. See docs/background-tick.md. MUST touch no `Context`/surface state — everything here is pure `self`.
    pub fn advance_protocol(&mut self, now: Instant) -> bool {
        let mut needs_redraw = false;

        // Log any tick phase that blocks the UI thread > 50ms so a stall shows up in the trace rather than being guessed at. Hoisted to the top of the tick so the phonebook-harvest/persist block below is covered too (the persist freeze lived in exactly the uninstrumented gap before this).
        macro_rules! timed {
            ($label:literal, $body:expr) => {{
                let __t = Instant::now();
                let __r = $body;
                let __ms = __t.elapsed().as_millis();
                if __ms > 50 {
                    crate::logf!("PERF: {} took {}ms (UI thread)", $label, __ms);
                }
                __r
            }};
        }

        // Mirror the calibration doctrine to the engine thread: an uncalibrated route never transmits (engine mic gate). Cheap per tick; follows route changes on their mirror edge.
        crate::call::ROUTE_CALIBRATED.store(
            self.route_calibrated_now(),
            std::sync::atomic::Ordering::Relaxed,
        );

        // DEMAND-DRIVEN presence (Nick 2026-09-02: "zero reason I need the status of my fleet OR friends until I open my fleet page OR my contacts screen; on a chat, ping THEM"): pings + the beacon that rides the full sweep fire only while the human is LOOKING at a presence surface — the contacts screen or the Fleet page (full sweep) or an open conversation (that one contact). Everything else arrives on edges: inbound acks/pongs/messages update state passively, the announce-on-online edge keeps our own mapping findable, retransmit ladders stay work-driven, and Android's FCM/doorbell is the background wake path. Screen ENTRY is an edge (immediate ping); while the surface stays visible a slow 30s refresh keeps the rings honest. Backgrounded = silent.
        {
            let watching = crate::platform::app_watching();
            let surface = match self.state {
                AppState::Ready => watching.then_some(0u8),
                AppState::Conversation => watching.then_some(1),
                AppState::Settings(SettingsPage::Fleet) => watching.then_some(2),
                _ => None,
            };
            if surface != self.presence_surface {
                self.presence_surface = surface;
                if surface.is_some() {
                    self.last_presence_ping = None; // entry edge → ping now
                }
            }
            if let Some(kind) = surface {
                const VISIBLE_REFRESH: std::time::Duration = std::time::Duration::from_secs(30);
                let due = self
                    .last_presence_ping
                    .is_none_or(|last| now.duration_since(last) >= VISIBLE_REFRESH);
                if due {
                    self.last_presence_ping = Some(now);
                    if kind == 1 {
                        self.ping_active_contact();
                    } else {
                        self.ping_contacts();
                    }
                }
            }
            // ALWAYS-ON LAN BEACON (regression fix 2026-09-02): the demand-driven gate above stopped a BACKGROUNDED device from announcing at all — so a fleet sibling on any non-presence screen went un-findable on the LAN, its address staled, and a self-message to it sprayed dead endpoints thru PT's retransmit ladder (~10s). The LAN multicast is ONE cheap subnet packet (not the unicast ping fan-out, not the WAN punch/relay machinery — those stay demand-driven and off in the background); keeping it on a modest cadence is what makes same-room delivery instant. Fires regardless of foreground while online.
            const LAN_BEACON: std::time::Duration = std::time::Duration::from_secs(20);
            let beacon_due = self
                .last_lan_beacon
                .is_none_or(|last| now.duration_since(last) >= LAN_BEACON);
            if self.online && beacon_due && surface.is_none() {
                if let (Some(session), Some(hq), Some(checker)) = (
                    self.session.as_ref(),
                    self.handle_query.as_ref(),
                    self.status_checker.as_ref(),
                ) {
                    self.last_lan_beacon = Some(now);
                    checker.send_lan_broadcast(session.handle_proof, hq.port());
                }
            } else if surface.is_some() {
                // A watched surface already beaconed via the ping sweep — keep the timer in step so leaving the surface doesn't double-fire.
                self.last_lan_beacon = Some(now);
            }
        }

        // Periodic OWN-chain re-fold — the reliable doorbell for fleet membership changes (docs/pairing-v2.md). The hub `fleet` event is the instant path but best-effort; this catches a device add/remove that arrived while our WebSocket was down. Reconciling siblings re-seeds the answerable-pubkey set, so a newly-added device starts getting pong answers (stops showing offline) and appears in the Fleet list without a relaunch. 45s: brisk enough that a just-added device goes live within a sweep, slow enough to be a negligible one-fetch background poll.
        const FLEET_REFOLD_INTERVAL: std::time::Duration = std::time::Duration::from_secs(45);
        // Demand-driven (2026-09-02): the refold poll runs only while a Settings page is open (the Fleet list is what it feeds); the hub `fleet` event remains the instant background path, and app-start does one refold via last_fleet_refold starting None.
        if matches!(self.state, AppState::Settings(_)) && crate::platform::app_watching() {
            let due = self
                .last_fleet_refold
                .is_none_or(|last| now.duration_since(last) >= FLEET_REFOLD_INTERVAL);
            if due {
                if let Some(our_hp) = self.our_handle_proof() {
                    self.last_fleet_refold = Some(now);
                    self.spawn_contact_fleet_refresh(vec![our_hp]);
                    // Roster-pull backstop (B4): an exhausted pull used to wait on "the next fleet event", which never comes with the WebSocket down — no roster until relaunch. This edge re-arms a small budget at the refold cadence; success clears the flag, failure re-exhausts and the next sweep tries again.
                    if self.roster_pull_exhausted && self.roster_pull_rx.is_none() {
                        self.roster_pull_exhausted = false;
                        self.needs_initial_roster_pull = true;
                        self.roster_pull_retries_left = 2;
                        crate::log(
                            "FLEET: refold edge re-arming the exhausted roster pull (2 attempts)",
                        );
                    }
                }
            }
        }

        // The checkpoint spine driver rides the same fleet sweep cadence: lazy vault load, outcome drain, and the bootstrap/rotation mint edges.
        if matches!(
            self.state,
            AppState::Ready | AppState::Conversation | AppState::Settings(_)
        ) {
            self.ckpt_tick();
        }

        // Stalled-address re-fetch — the deadlock breaker for flaky-fgtw address discovery.
        // A contact whose address fetch failed sits with `ip = None`: its CLUTCH offer can't send (send needs an address), name/avatar never arrive (they ride the pong, which needs a reachable path), and the ceremony loops keygen forever. There is no periodic address re-fetch otherwise, so while any non-self contact is Pending-CLUTCH with no address, pulse a lightweight background resume (gossip + registry resolve below). A single success learns the address, fire-on-learn punches, the offer sends, and the pong then carries name/avatar. Self-limiting: stops the moment the address lands. (Stopgap for the peer-gossip fix, TICKETS T0.)
        const STALLED_ADDR_REFETCH: std::time::Duration = std::time::Duration::from_secs(15);
        // EXPONENTIAL BACKOFF on the refetch (battery, 2026-09-02): a Pending contact that never resolves (era-split debris) ran this full registry+punch cycle every 15s for hours. 15s→30s→…→~32min; any LEARNED address resets the streak (the success edge), as does an address-bearing gossip merge re-entering thru the same harvest.
        if matches!(
            self.state,
            AppState::Ready | AppState::Conversation | AppState::Settings(_)
        ) {
            let blocked = self.contacts.iter().any(|c| {
                c.ip.is_none()
                    && c.clutch_state == crate::types::ClutchState::Pending
                    && self.has_remote(c)
            });
            let interval = STALLED_ADDR_REFETCH * (1u32 << self.stalled_refetch_streak.min(7));
            let due = self
                .last_stalled_refetch
                .is_none_or(|last| now.duration_since(last) >= interval);
            // Harvest every tick while blocked: a record for a stalled contact may have landed in the shared peer store — from our own fgtw fetch OR from a phonebook-gossip response.
            // Adopt it as the contact's address so the offer can send; fire-on-learn does the rest.
            timed!("phonebook-harvest", if blocked {
                let recs = self
                    .peer_store
                    .as_ref()
                    .map(|s| s.lock().unwrap().get_all_peers())
                    .unwrap_or_default();
                if !recs.is_empty() {
                    let mut learned = false;
                    for contact in self.contacts.iter_mut() {
                        if contact.ip.is_some() {
                            continue;
                        }
                        if let Some(rec) = recs.iter().find(|r| {
                            r.handle_proof == contact.handle_proof
                                && Some(*r.device_pubkey.as_bytes()) == contact.device_key()
                        }) {
                            // Same refusal as the phonebook drain: a record signed before the bogus-address guard existed (or by a since-retired device that will never republish) can carry the unspecified address forever — adopting it points every send at 0.0.0.0 and the contact reads permanently offline.
                            if crate::network::traverse::gather::is_bogus_addr(&rec.ip) {
                                continue;
                            }
                            contact.ip = Some(rec.ip);
                            contact.punch_unvalidated_cycles = 0;
                            learned = true;
                            crate::logf!("GOSSIP/harvest: adopted a stalled contact's address from the peer store");
                        }
                    }
                    if learned {
                        self.stalled_refetch_streak = 0;
                        self.ping_contacts();
                    }
                }
                // Persist on the growth edge, observed here where the store is already in hand. Marks dirty; the tick's debounce gate does the off-thread write, coalescing a burst of gossip merges into one.
                if recs.len() != self.peer_store_persisted_len {
                    self.peer_store_persisted_len = recs.len();
                    self.request_peer_persist();
                }
            });
            // Every 15s while blocked: ask every reachable peer for its phonebook AND resolve the stalled devices from the seed registry — peers first, seed last. This used to also fire `query_resume`, which replays the ENTIRE attest (contacts load, cloud sync, roster pull, fleet key sync — 749 full replays in one logged session, and the roster-pull storm rode it via needs_initial_roster_pull). The resume's only job here was the announce echo that learned addresses, and the per-record registry resolve below does that properly now.
            if blocked && due {
                self.last_stalled_refetch = Some(now);
                self.stalled_refetch_streak = self.stalled_refetch_streak.saturating_add(1);
                crate::logf!("FGTW: a Pending contact has no address — gossiping reachable peers + resolving from the seed registry");
                let reachable: Vec<std::net::SocketAddr> = self
                    .contacts
                    .iter()
                    .filter_map(|c| c.validated_path.map(|(a, _)| a))
                    .collect();
                if let Some(checker) = self.status_checker.as_ref() {
                    for addr in reachable {
                        checker.send_phonebook_request(addr);
                    }
                }
                // PEERS FIRST, SEED LAST. The gossip request above asks everyone we can already reach; this asks the seed only for what that could not answer. On a cold start `reachable` is empty — no path is validated because no address is known — and the seed is the only party reachable without knowing anyone, so it is what re-enters the cycle.
                self.resolve_stalled_addresses_from_seed();
            }
            // Apply whatever the seed answered (a resolve spawned on an earlier tick).
            if self.drain_pb_resolve() {
                needs_redraw = true;
            }
            // Deferred wire half of sends whose bubbles rendered last frame.
            if self.drain_pending_chain_sends() {
                needs_redraw = true;
            }
        }

        // Drain per-contact presence + CLUTCH ceremony updates (pongs → is_online/ip; offers/KEM/complete → ceremony progress), plus the three background-job result channels (keygen / KEM-encap / ceremony-expand). The `timed!` macro (hoisted to the top of this fn) logs any arm blocking the UI thread > 50ms.
        if timed!("check_status_updates", self.check_status_updates()) {
            needs_redraw = true;
        }
        // Ring colours are DERIVED state — recompute and diff every tick, repaint on any change (see painted_ring_tiers).
        {
            let tiers: Vec<u32> = self
                .contacts
                .iter()
                .map(|c| self.row_ring_tier(c))
                .collect();
            if tiers != self.painted_ring_tiers {
                self.painted_ring_tiers = tiers;
                needs_redraw = true;
                // Marked here, not via the return: the Android service tick runs this headless and drops the bool — but it still updated painted_ring_tiers above, so on resume the diff read "already painted" and the ring stayed stale until a click (field, 2026-08-08). The dirty flag is state; it survives to the first visible frame.
                self.scene_dirty = true;
            }
        }
        if timed!("check_clutch_keygens", self.check_clutch_keygens()) {
            needs_redraw = true;
        }
        // Serialized keygen queue: once the in-flight keygen (if any) has completed and cleared its flag above, start the next Pending-keyless contact's keygen. One McEliece at a time keeps the UI responsive on a multi-contact launch instead of spawning them all at once.
        timed!(
            "spawn_next_pending_keygen",
            self.spawn_next_pending_keygen()
        );
        if timed!("check_clutch_kem_encaps", self.check_clutch_kem_encaps()) {
            needs_redraw = true;
        }
        if timed!("check_clutch_kem_decaps", self.check_clutch_kem_decaps()) {
            needs_redraw = true;
        }
        if timed!("check_clutch_ceremonies", self.check_clutch_ceremonies()) {
            needs_redraw = true;
        }

        // Deferred own-avatar recovery: the pin arrives with the fleet settings, after the session restore that wanted it.
        if let Some(seed) = self.self_avatar_recover_pending {
            if self.device_avatar_pixels.is_some() || self.spawn_self_avatar_recover(seed) {
                self.self_avatar_recover_pending = None;
            }
        }

        // Freshly re-minted reader secrets need a slot at their new address (see the mint edge). Off-thread: each grant is a blocking upload.
        if !self.scoped_regrant_pending.is_empty() {
            let secrets = std::mem::take(&mut self.scoped_regrant_pending);
            if let (Some(kp), Some(hp), Some(our_pid), Some(storage)) = (
                self.device_keypair.clone(),
                self.our_handle_proof(),
                self.session
                    .as_ref()
                    .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)),
                self.storage.as_ref().map(Arc::clone),
            ) {
                std::thread::spawn(move || {
                    // A grant serves OUR published blob, so the purpose carries OUR pid (the publisher).
                    let purpose = crate::ui::avatar_scoped::avatar_purpose(&our_pid);
                    for kek in secrets {
                        crate::ui::avatar_scoped::grant_reader(&kek, &purpose, &kp, &hp, &storage);
                    }
                });
            }
        }

        // Keep any notes-to-self row showing OUR name and avatar — a profile edit or a fresh avatar must reach it, and nothing else ever will (no peer pongs us).
        self.settle_self_display();

        // A sibling pair just became egged — a consent edge worth a GROW so its wrap exists without waiting for the sponsor's confirm path (docs/fleet-key.md: growth never mints). Edge-driven off the ceremony drain, never polled.
        if std::mem::take(&mut self.fanout_grow_pending) {
            crate::log("FANOUT: newly egged sibling — growing so it gets a wrap");
            self.spawn_fleet_key_grow();
        }

        // Drain handle_query results. `try_recv` is non-blocking; we collect into local Vecs so the immutable borrow on `handle_query` ends before the `&mut self` handlers run. Three channels feed in: attestation results, connectivity changes, handle searches.
        let mut drained: Vec<QueryResult> = Vec::new();
        let mut drained_searches: Vec<crate::ui::state::SearchResult> = Vec::new();
        if let Some(hq) = self.handle_query.as_ref() {
            while let Some(result) = hq.try_recv() {
                drained.push(result);
            }
            while let Some(online) = hq.try_recv_online() {
                self.online = online;
                // Only drive the SELF-connectivity ring when the orb isn't currently a peer's avatar (a conversation owns the orb via update_orb); otherwise this would strobe our green/red over their presence ring.
                if self.orb_contact.is_none() {
                    if let Some(chrome) = self.chrome.as_mut() {
                        chrome.set_orb_tint(orb_tint_for(online));
                    }
                }
                needs_redraw = true;
            }
            while let Some(search) = hq.try_recv_search() {
                drained_searches.push(search);
            }
        }
        for result in drained {
            timed!("on_query_result", self.on_query_result(result));
            needs_redraw = true;
        }
        for search in drained_searches {
            self.on_search_result(search);
            needs_redraw = true;
        }

        // AddDevice flow: apply off-thread match-check/bind results (drain first so the rx borrow ends before we mutate self).
        let add_updates: Vec<AddDeviceUpdate> = self
            .add_device_rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for update in add_updates {
            match update {
                AddDeviceUpdate::Candidates(reqs) => {
                    // Precompute each candidate's expected word tokens + keyed name once per refresh, so the per-keystroke matcher is a plain string walk. Requests were already signature-verified in bindreq_list; the seed is in-session by definition on this screen. `heard_ble` marks candidates whose beacon we're hearing right now (proximity) — resolved by matching each heard service UUID's keyed tag to the candidate's pubkey under our fleet key.
                    if let Some((seed, hp)) = self
                        .session
                        .as_ref()
                        .map(|s| (s.identity_seed, s.handle_proof))
                    {
                        use crate::network::fgtw::fleet;
                        let heard = crate::network::pairing_beacon::heard();
                        let now = std::time::Instant::now();
                        self.lan_heard.retain(|(_, t)| now.duration_since(*t) < LAN_HEARD_FRESH);
                        let lan_heard = self.lan_heard.clone();
                        self.add_device_candidates = reqs
                            .into_iter()
                            .map(|req| {
                                let words = fleet::masked_device_words(&req.device_pubkey, &seed);
                                AddCandidate {
                                    name: fleet::device_name_default(&req.device_pubkey, &seed),
                                    tokens: fleet::pair_word_list(&words),
                                    // Recompute this candidate's beacon id from its OWN published (pubkey, eagle_time) under our handle key; a heard match = proximity.
                                    heard_ble: heard.iter().any(|b| {
                                        fgtw::pair::beacon_matches(
                                            &b.uuid,
                                            &hp,
                                            &req.device_pubkey,
                                            req.t,
                                        )
                                    }),
                                    // LAN proximity: the candidate's own pubkey heard broadcasting under our handle on this network.
                                    heard_lan: lan_heard.iter().any(|(k, _)| *k == req.device_pubkey),
                                    req,
                                }
                            })
                            .collect();
                        self.refresh_add_device_match();
                    }
                }
                AddDeviceUpdate::Bound(pk) => {
                    self.add_device_checking = false;
                    self.add_device_bound = Some(pk);
                    let name = self
                        .session
                        .as_ref()
                        .map(|s| {
                            crate::network::fgtw::fleet::device_name_default(&pk, &s.identity_seed)
                        })
                        .unwrap_or_default();
                    if self.add_device_bind_ble {
                        // BLE / list-tap select: the candidate was picked by proximity + name, NOT by typing its full 256-bit key — so a wrong pick is possible. Hold the fleet-key rotation behind the human's "did it turn green?" confirm (two-phase); a wrong bind stays a keyless ledger entry.
                        self.add_device_status = tr(Msg::BoundDeviceConfirm(&name)).into_owned();
                    } else {
                        // WORDS path: the typed 256-bit match already IS the confirmation (you can only type the words shown on the one device in your hand — no wrong candidate), so release the fleet key immediately.
                        self.add_device_status = tr(Msg::AddingDevice(&name)).into_owned();
                        self.spawn_confirm_add();
                    }
                }
                AddDeviceUpdate::Rotated => {
                    self.add_device_checking = false;
                    // Ceremony complete — back to the Fleet page it was launched from (the new device's row is the confirmation), instead of stranding the user on a finished words screen.
                    self.end_add_device_flow();
                    self.refresh_fleet_retired();
                    self.state = AppState::Settings(SettingsPage::Fleet);
                    self.ready_toast = Some(tr(Msg::DeviceAdded).into_owned());
                    // The confirm rotated the fleet key — recover the new epoch AND re-seal the roster under it in one ordered pass, so the just-joined device's roster pull decrypts instead of failing aead::Error until a relaunch. (Was a bare key-sync that left the roster stale-sealed forever, since the periodic re-push only fires on a non-in-app attest.)
                    self.spawn_roster_republish();
                    // And re-fold our own chain immediately so the freshly-bound device gets its sibling contact (fleet weave kickoff) without waiting for the next fleet event.
                    if let Some(our_hp) = self.our_handle_proof() {
                        self.spawn_contact_fleet_refresh(vec![our_hp]);
                    }
                }
                AddDeviceUpdate::Failed(e) => {
                    self.add_device_checking = false;
                    self.add_device_status = tr(Msg::AddDeviceError(&e)).into_owned();
                }
            }
            needs_redraw = true;
        }

        // Diagnostics log-submit results (off-thread FGTW upload).
        let log_submit_updates: Vec<Result<(), String>> = self
            .log_submit_rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for update in log_submit_updates {
            self.log_submit_inflight = false;
            match update {
                Ok(()) => {
                    self.ready_toast = Some(tr(Msg::LogSent).into_owned());
                    // Clear the note once it's been submitted so the next submit starts blank. MUST be the widget's clear() — wiping `chars` directly leaves cursor/widths stale, and the next cursor paint slices widths[..cursor] out of range → panic → abort (this was the "submit a log → app dies" crash).
                    if let Some(tb) = self.settings_note_textbox.as_mut() {
                        tb.clear();
                    }
                    crate::log("DIAG: log submitted to FGTW");
                    // Clear on success, so the next submit carries only what happened SINCE. Submitting and then clearing by hand was the ritual this replaces, and forgetting it re-sends everything already on the server — duplicate lines that make the next log harder to read, not easier. The copy that matters is on FGTW; the local file's only remaining job is to accumulate what comes next.
                    crate::clear_log();
                    // Baseline AFTER the clear so the pill greys until something genuinely new lands.
                    self.log_submitted_len = Some(crate::log_size_bytes());
                }
                Err(e) => {
                    self.ready_toast = Some(tr(Msg::SendFailed(&e)).into_owned());
                    crate::logf!("DIAG: log submit failed: {}", e);
                }
            }
            needs_redraw = true;
        }

        // The auto-update checkbox is the first linked-settings consumer: a user toggle writes updates.auto (born linked, so the whole fleet follows; unlink comes with the per-setting link affordance). Poll-then-set keeps the borrow simple.
        let autoupdate_toggle = self
            .settings_autoupdate_check
            .as_mut()
            .map(|cb| (cb.take_toggle(), cb.is_checked()));
        if let Some((true, checked)) = autoupdate_toggle {
            if self.settings_set("updates.auto", vsf::VsfType::u0(checked)) {
                crate::logf!("SETTINGS: updates.auto = {} (linked write)", checked);
            }
            needs_redraw = true;
        }

        // Vibrate/ring toggles (Notifications): fleet-wide linked writes. Enforcement is the alert paths' side (ring gate in ring_alert; vibration rides Kotlin — follow-up).
        // Poll all three first (each take_toggle borrows self mutably), then write — settings_set needs &mut self too.
        let vib_msg = self
            .settings_vibrate_msg_check
            .as_mut()
            .map(|c| (c.take_toggle(), c.is_checked()));
        let ring_call = self
            .settings_ring_call_check
            .as_mut()
            .map(|c| (c.take_toggle(), c.is_checked()));
        let vib_call = self
            .settings_vibrate_call_check
            .as_mut()
            .map(|c| (c.take_toggle(), c.is_checked()));
        for (toggle, key) in [
            (vib_msg, "notify.vibrate_msg"),
            (ring_call, "notify.ring_call"),
            (vib_call, "notify.vibrate_call"),
        ] {
            if let Some((true, checked)) = toggle {
                if self.settings_set(key, vsf::VsfType::u0(checked)) {
                    crate::logf!("SETTINGS: {} = {} (linked write)", key, checked);
                }
                needs_redraw = true;
            }
        }

        // Dozenal toggle (About page): fleet-wide linked write + the render-edge static mirror flips NOW so every number on screen switches base this frame.
        let dozenal_toggle = self
            .settings_dozenal_check
            .as_mut()
            .map(|cb| (cb.take_toggle(), cb.is_checked()));
        if let Some((true, checked)) = dozenal_toggle {
            crate::set_dozenal_ui(checked);
            if self.settings_set("display.dozenal", vsf::VsfType::u0(checked)) {
                crate::logf!("SETTINGS: display.dozenal = {} (linked write)", checked);
            }
            self.scene_dirty = true;
            needs_redraw = true;
        }

        // Hard-logs toggle: arm THIS device for 24h (the value stored is the arm time; the sink self-expires) — device-local via unlink, mirroring the display.zoom pattern. Arming flips the sink NOW (a flush edge).
        let hardlogs_toggle = self
            .settings_hardlogs_check
            .as_mut()
            .map(|cb| (cb.take_toggle(), cb.is_checked()));
        if let Some((true, checked)) = hardlogs_toggle {
            let now = vsf::eagle_time_oscillations();
            crate::set_hard_logs(checked.then_some(now));
            if self.ensure_fleet_settings() {
                let fs = self.fleet_settings.as_mut().unwrap();
                if fs.linked("logs.hard") {
                    fs.set_link("logs.hard", false, now);
                }
                // Armed = the arm TIME (a timestamp is an e); disarmed = an honest u0(false).
                let val = if checked {
                    vsf::VsfType::e(vsf::types::EtType::e6(now))
                } else {
                    vsf::VsfType::u0(false)
                };
                if fs.set("logs.hard", val, now) {
                    crate::logf!(
                        "SETTINGS: logs.hard {} (device-local, 24h self-expiry)",
                        if checked { "armed" } else { "disarmed" }
                    );
                    self.persist_and_push_settings();
                }
            }
            needs_redraw = true;
        }

        // You-page default-share toggles: checked = the field auto-shares with NEW contacts (the always-shared display name has no box). Poll-then-set keeps the borrow simple; the key syncs fleet-wide like the value it gates.
        let share_writes: Vec<(String, bool)> = self
            .you_fields
            .iter_mut()
            .filter_map(|pf| {
                let cb = pf.share_cb.as_mut()?;
                cb.take_toggle()
                    .then(|| (pf.field_id.clone(), cb.is_checked()))
            })
            .collect();
        for (fid, checked) in share_writes {
            if self.settings_set(&format!("share.{fid}"), vsf::VsfType::u0(checked)) {
                crate::logf!("SETTINGS: share.{} = {} (default-share)", fid, checked);
            }
            needs_redraw = true;
        }

        // Desktop resident-mode toggle: the OS autostart artifact IS the stored setting (platform::autostart — nothing in the vault to desync), and the live flag follows it immediately, so unchecking makes the very next close a real quit. A write failure reverts the box and says why.
        #[cfg(not(target_os = "android"))]
        {
            let bg_toggle = self
                .settings_background_check
                .as_mut()
                .map(|cb| (cb.take_toggle(), cb.is_checked()));
            if let Some((true, checked)) = bg_toggle {
                let result = if checked {
                    crate::platform::autostart::enable()
                } else {
                    crate::platform::autostart::disable()
                };
                match result {
                    Ok(()) => {
                        // The veto marker is the durable half of the choice — background is default-ON, so "off" must survive restarts (the artifact alone would just re-enroll next launch).
                        crate::platform::autostart::set_background_desired(checked);
                        self.resident_mode = checked;
                        crate::logf!(
                            "RESIDENT: background mode {} (login item {})",
                            if checked { "ON" } else { "OFF" },
                            if checked { "written" } else { "removed" }
                        );
                        if checked && !self.tray_spawned {
                            if let Some(proxy) = self.event_proxy.clone() {
                                crate::platform::tray::spawn(proxy);
                                self.tray_spawned = true;
                            }
                        }
                    }
                    Err(e) => {
                        crate::logf!("RESIDENT: login-item change failed: {}", e);
                        if let Some(cb) = self.settings_background_check.as_mut() {
                            cb.set_checked(!checked);
                        }
                        self.ready_toast = Some(tr(Msg::CouldntChangeLoginItem(&e)).into_owned());
                    }
                }
                needs_redraw = true;
            }
        }

        // Clear the "handle didn't match" line as soon as the operator edits the confirm box again — event-shown, interaction-cleared, no timers.
        if self.unattended_confirm_failed {
            let has_text = self
                .unattended_confirm_tb
                .as_ref()
                .map(|tb| !tb.chars.is_empty())
                .unwrap_or(false);
            if has_text {
                self.unattended_confirm_failed = false;
                needs_redraw = true;
            }
        }

        // Unattended (auto-attest-on-reboot) toggle: a flip does NOT act — it opens the handle-confirmation modal (arming AND disarming this device-becomes-you switch must re-prove the operator, not just whoever reached the unlocked screen). The visual box is reverted immediately; the modal's verified confirm is what actually writes state.
        let unattended_toggle = self
            .settings_unattended_check
            .as_mut()
            .map(|cb| (cb.take_toggle(), cb.is_checked()));
        if let Some((true, checked)) = unattended_toggle {
            // Revert the box to the true current state; the modal owns the real change.
            if let Some(cb) = self.settings_unattended_check.as_mut() {
                cb.set_checked(Self::unattended_enabled());
            }
            self.unattended_confirm = Some(checked); // target_on = where the flip wanted to go
            self.unattended_confirm_failed = false;
            if let Some(tb) = self.unattended_confirm_tb.as_mut() {
                tb.clear();
                let id = tb.hit_id();
                self.change_focus(Some(id));
            }
            needs_redraw = true;
        }

        // AddDevice flow: the status line is EVENT-driven, re-derived on every edit by the LIVE MATCHER — the typed entry prefix-matches against the candidate word strings from the binding-request registry (docs/pairing-v2.md), so a typo flags at the exact word it happens and a full 23-word match auto-binds.
        if matches!(self.state, AppState::AddDevice) {
            let text: String = self
                .textbox
                .as_ref()
                .map(|tb| tb.chars.iter().collect())
                .unwrap_or_default();
            if text != self.add_device_wordcheck_text {
                self.add_device_wordcheck_text = text;
                self.refresh_add_device_match();
                needs_redraw = true;
            }
        }

        // New-device JOIN flow: words display + matched flag + membership results.
        let join_updates: Vec<JoinUpdate> = self
            .add_join_rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for update in join_updates {
            match update {
                JoinUpdate::ShowWords(words) => {
                    self.add_join_words = Some(words);
                    self.add_join_status = tr(Msg::AddFromSignedIn).into_owned();
                }
                JoinUpdate::Joined(fleet_key, session) => {
                    if fleet_key.is_none() {
                        // JOINER SELECTED (docs/lifecycle.md): bound into the chain but the sponsor's human hasn't confirmed — THIS screen going green IS what they're being asked to verify. Flood green, say "Selected!", and HOLD; sign-in fires when the confirm rotation releases the fleet key. The poller re-emits Joined(Some(key)) thru a fresh channel, so the Some-branch below is the single sign-in path.
                        crate::log(
                            "JOIN: bound — GREEN (Selected!), holding for the sponsor's confirm",
                        );
                        self.add_join_words = None;
                        self.add_join_status.clear();
                        self.joiner_selected = true;
                        self.scene_dirty = true;
                        let (tx, rx) = std::sync::mpsc::channel();
                        self.add_join_rx = Some(rx);
                        let hp = session.handle_proof;
                        let kp = self.device_keypair.clone();
                        let wake = self.event_proxy.clone();
                        std::thread::spawn(move || {
                            let Some(kp) = kp else { return };
                            // ~15 min of 2s polls — the sponsor is a human mid-tap, not a batch job; past this the ceremony is abandoned and a relaunch re-joins cleanly.
                            for _ in 0..(15 * 30) {
                                std::thread::sleep(std::time::Duration::from_secs(2));
                                if let Ok(Some(k)) = crate::network::fgtw::fleet::recover_fleet_key(
                                    &hp,
                                    &kp,
                                    &session.identity_seed,
                                ) {
                                    if tx.send(JoinUpdate::Joined(Some(k), session)).is_err() {
                                        return; // screen left — nobody waiting
                                    }
                                    if let Some(w) = wake.as_ref() {
                                        let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
                                    }
                                    return;
                                }
                            }
                        });
                        needs_redraw = true;
                        continue;
                    }
                    // The confirm landed — leave the green hold and run the normal attest (it now passes the fleet gate). Stash the fleet key to persist once attest sets the vault up.
                    self.joiner_selected = false;
                    self.add_join_rx = None;
                    self.launch_add_mode = false;
                    self.add_join_words = None;
                    self.add_join_status.clear();
                    self.pending_fleet_key = fleet_key;
                    self.add_join_handle = None;
                    // Attest with the roots the join thread already derived — no handle re-entry, no second ~1s proof, and no route thru submit_handle's permanence interstitial (this claims nothing new; the fleet exists and we were just bound into it).
                    if let Some(hq) = self.handle_query.as_ref() {
                        hq.query_first_attest_with_roots(session);
                        self.state = AppState::Launch(LaunchState::Attesting);
                        self.change_focus(None);
                    }
                }
                JoinUpdate::Failed(e) => {
                    // The ceremony is dead — take the words DOWN with it. Leaving them up strands the screen on a corpse: the user keeps waiting on words no thread is polling for. Back to handle entry with the error visible; re-submitting starts a fresh ceremony.
                    self.add_join_rx = None;
                    self.add_join_words = None;
                    self.add_join_status = tr(Msg::JoinFailed(&e)).into_owned();
                }
            }
            needs_redraw = true;
        }

        // Deferred initial roster pull: fire the moment the (async-synced) fleet key lands, so wake-up catch-up brings sibling-added friends onto this device. One-shot per attest/resume.
        if self.needs_initial_roster_pull
            && self.roster_pull_rx.is_none()
            && self.fleet_key_cached().is_some()
        {
            self.needs_initial_roster_pull = false;
            crate::log("FLEET: initial roster pull (wake-up catch-up)");
            self.spawn_roster_pull();
        }

        // Removal-heal follow-up, winner only (braid.md §14.2): our heal thread won a rotation, so revoke the one fleet-held bearer credential OUTSIDE the fstate slot — the avatar pin. Losers adopted the winner's key off-thread and have nothing to do here.
        if self.fleet_rotated_rx.try_iter().count() > 0 {
            self.rotate_avatar_pin();
            // A won rotation is the PCS boundary — fold the fresh fleet key into the epoch spine NOW, not up to a full row-cadence later.
            self.ckpt_mint_due = true;
        }

        // Fleet roster pull result: merge into the contact list (re-CLUTCH happens via the serialized keygen kick inside merge_roster_entries). Fleet-event push: a sibling device changed the shared roster (fstate) or the membership chain (fleet) — pull the change NOW instead of at our next attest. This is what makes a friend added on one device appear on the rest of the fleet in about a second.
        let fleet_evts: Vec<(&'static str, [u8; 32])> = self
            .fleet_evt_rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        if !fleet_evts.is_empty() {
            let our_hp = self.our_handle_proof();
            let mut refresh_contacts: Vec<[u8; 32]> = Vec::new();
            for (kind, evt_hp) in &fleet_evts {
                if *kind == "release" {
                    // Release notice off the hub — the advisory "go look" a deploy fires: make the next drive_auto_update poll due NOW instead of the jittered 6-8h cadence. Trust is unchanged — the poll fetches the SIGNED manifest thru the stamp window, so the worst a forged notice does is cause a poll. (`1` = due-in-the-past but nonzero, so the ==0 first-launch ramp doesn't swallow it.)
                    crate::log("UPDATE: release notice from the hub — manifest poll due now");
                    self.next_update_check_osc = 1;
                    continue;
                }
                if Some(*evt_hp) == our_hp {
                    // OUR fleet: shared-state or membership change — pull it now.
                    match *kind {
                        "fstate" | "friendship" if self.roster_pull_rx.is_none() => {
                            self.spawn_roster_pull()
                        }
                        "fleet" => {
                            self.spawn_fleet_key_sync();
                            // Membership changed: re-fold our own chain so sibling contacts reconcile (fleet weave) — this is how existing members learn about a freshly-added device within ~a second.
                            if !refresh_contacts.contains(evt_hp) {
                                refresh_contacts.push(*evt_hp);
                            }
                        }
                        _ => {}
                    }
                } else if *kind == "fleet"
                    && self.contacts.iter().any(|c| c.handle_proof == *evt_hp)
                    && !refresh_contacts.contains(evt_hp)
                {
                    // A CONTACT's fleet chain extended (they added/removed a device) — re-fold so we honour their current device set.
                    refresh_contacts.push(*evt_hp);
                }
            }
            if !refresh_contacts.is_empty() {
                self.spawn_contact_fleet_refresh(refresh_contacts);
            }
            needs_redraw = true;
        }

        // Contact-fleet refresh results: fold-and-honour a friend's current device set, and ARM the fold-respecting trust rule. OUR OWN hp routes to sibling reconcile FIRST and never into any contact's fleet_members — the self-contact and every sibling contact carry our hp, and folding our own fleet into one of them would make it swallow sibling pongs/paths via first-match `knows_device` routing.
        let member_updates: Vec<([u8; 32], Vec<[u8; 32]>, i64, [u8; 32], bool)> = self
            .contact_members_rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        if !member_updates.is_empty() {
            let our_hp = self.our_handle_proof();
            let our_device = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes());
            let siblings = sibling_presence_snapshot(&self.contacts);
            let mut changed = false;
            let mut to_persist: Vec<usize> = Vec::new();
            let mut successor_checks: Vec<([u8; 32], [u8; 32])> = Vec::new();
            for (hp, members, tip_ts, genesis, existed) in member_updates {
                if Some(hp) == our_hp {
                    self.reconcile_fleet_siblings(&members);
                    // Cutover: OUR fold is the primary registry's truth — converge on the fold-change edge (any member may; writes are idempotent and epoch-guarded, so racing siblings settle).
                    if self.registry_converged_fold != members {
                        self.registry_converged_fold = members.clone();
                        self.spawn_registry_converge(hp, members.clone());
                    }
                    needs_redraw = true;
                    continue;
                }
                let Some((idx, c)) = self
                    .contacts
                    .iter_mut()
                    .enumerate()
                    .find(|(_, c)| c.handle_proof == hp)
                else {
                    continue;
                };
                // IDENTITY ENDED (docs/lifecycle.md D3): the chain VANISHED for a contact we had folded — the owner's last departure purged it. Freeze everything (verify-or-withhold: never destroy local state on a not_found; a lying worker must not fake a death) and render the contact as ended. A reappearing chain routes thru the genesis-pin check below: same genesis clears the flag (worker blip), different genesis = a successor.
                if !existed {
                    if c.fleet_folded_once && !c.identity_ended {
                        crate::logf!("FLEET: {}'s chain is GONE — identity ended by its owner; freezing the contact", crate::fp(&hp));
                        c.identity_ended = true;
                        changed = true;
                        to_persist.push(idx);
                    }
                    continue;
                }
                // GENESIS PIN (docs/lifecycle.md — free must not mean inheritable): first adopted fold pins the generation id; any later fold whose genesis differs is a SUCCESSOR holding a re-claimed name — the same party id derives from the same handle string, so WITHOUT this pin the impostor would inherit the friendship. Refuse the fold, mark superseded, render a stranger. Never overwrite the pin.
                if c.pinned_genesis != [0u8; 32] && c.pinned_genesis != genesis {
                    if !c.identity_superseded {
                        crate::logf!("FLEET: {}'s chain has a DIFFERENT genesis — the name was re-claimed by someone else; refusing the fold, rendering a stranger", crate::fp(&hp));
                        c.identity_superseded = true;
                        changed = true;
                        to_persist.push(idx);
                    }
                    // A different genesis is EITHER an impostor OR a legitimate re-found by the same owner. Probe the succession slot off-thread: only a record with a continuity egg from a PREDECESSOR member verifies against our pin, so an impostor can't force the migration. Collected here, spawned after the loop (can't borrow self while holding `c`); the in-flight guard is applied there so a persisting mismatch probes at most once at a time but re-probes on a later refresh (record may publish late).
                    successor_checks.push((hp, c.pinned_genesis));
                    continue;
                }
                if c.identity_ended {
                    // Same genesis came back — the not_found was a blip, not a death.
                    crate::logf!(
                        "FLEET: {}'s chain is back (same genesis) — un-ending",
                        crate::fp(&hp)
                    );
                    c.identity_ended = false;
                    changed = true;
                }
                // Monotonic freshness gate FIRST, before any mutation: never adopt a fold whose tip is older than the last one we adopted (an R2 eventual-consistency read serving a stale pre-removal set must not overwrite a fresh post-removal one). A first fold (fleet_members_ts == 0) always passes since real eagle times are positive.
                if c.fleet_folded_once && tip_ts < c.fleet_members_ts {
                    crate::logf!(
                        "FLEET: ignoring stale fold for {} (tip {} < adopted {})",
                        crate::fp(&hp),
                        tip_ts,
                        c.fleet_members_ts
                    );
                    continue;
                }
                let shrank =
                    c.fleet_folded_once && c.fleet_members.iter().any(|m| !members.contains(m));
                let grew = members.iter().any(|m| !c.fleet_members.contains(m));
                let set_changed = c.fleet_members != members;
                let arming = !c.fleet_folded_once;
                if set_changed || arming || tip_ts != c.fleet_members_ts {
                    c.fleet_members = members;
                    c.fleet_members_ts = tip_ts;
                    c.fleet_folded_once = true; // armed ONLY here, on an adopted fold — never on Err/stale
                    if c.pinned_genesis == [0u8; 32] {
                        c.pinned_genesis = genesis; // first-met pin: the generation this friendship belongs to
                    }
                    changed = changed || set_changed || arming;
                    to_persist.push(idx);
                    if shrank {
                        crate::logf!("FLEET: device revoked from {}'s fleet — dropping it from the answerable set", crate::fp(&hp));
                        // Cutover: a departed device also loses its endpoint rows — keeping them would keep pinging and relaying to hardware the identity disowned (the fold is the truth; the registry pop-swaps it out on the owner's side).
                        let fold = c.fleet_members.clone();
                        c.device_endpoints.retain(|ep| fold.contains(&ep.pubkey));
                    }
                    // Fold-race self-heal: a peer's NEW device can drive the ceremony before we folded it — its CLUTCH SPEC was rejected as "not in contacts", and it gives up before our fold lands. Now that the fold makes that sibling answerable, re-arm our offer so the ceremony re-fires to it (prompting its KEM, which the PT gate now accepts). Only for an unfinished ceremony that grew a member — never disturb a Complete one.
                    if grew && c.clutch_state != crate::types::ClutchState::Complete {
                        // §4.2: only the ceremony owner re-arms — a parked sibling re-arming here would revive its competing round.
                        if ceremony_parked_by(c, our_device, &siblings) {
                            crate::logf!("CLUTCH: {} fleet grew but ceremony is parked — owner re-arms, not us", crate::fp(&hp));
                        } else {
                            c.clutch_offer_sent = false;
                            crate::logf!("CLUTCH: {} fleet grew mid-ceremony — re-arming offer to reach the folded device", crate::fp(&hp));
                        }
                    }
                }
            }
            if changed {
                self.reseed_contact_pubkeys(); // rebuild answerable set BEFORE persist: an in-flight pong this tick already sees the revoked device gone
                needs_redraw = true;
            }
            // Persist the adopted folded set + arm flag + tip ts so a restart resumes fold-respecting trust immediately (no bootstrap regression, no trust-nobody window).
            if !to_persist.is_empty() {
                if let Some(storage) = self.storage.as_ref().cloned() {
                    to_persist.sort_unstable();
                    to_persist.dedup();
                    for idx in to_persist {
                        if let Err(e) =
                            crate::storage::contacts::save_contact(&self.contacts[idx], &storage)
                        {
                            crate::logf!("FLEET: persist folded set failed: {}", e);
                        }
                    }
                }
            }
            // Succession probes for the mismatching folds — one network thread per contact, deduped by the in-flight guard (cleared when its result drains below).
            for (hp, pinned_genesis) in successor_checks {
                if !self.successor_inflight.contains(&hp) {
                    self.spawn_successor_check(hp, pinned_genesis);
                }
            }
        }

        // Drain succession probe results: a verified successor migrates the pin to the new genesis and un-supersedes the contact (its re-founded chain then re-folds on the next refresh); a `None` just clears the in-flight guard. Verification already happened off-thread against the pin, so a delivered `Some` is trustworthy here.
        let successor_results: Vec<([u8; 32], Option<[u8; 32]>)> = self
            .successor_rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for (hp, outcome) in successor_results {
            self.successor_inflight.remove(&hp);
            let Some(new_genesis) = outcome else { continue };
            let Some(idx) = self.contacts.iter().position(|c| c.handle_proof == hp) else {
                continue;
            };
            crate::logf!(
                "SUCCESSION: {} re-founded and verified against the pin — migrating the genesis pin, un-superseding",
                crate::fp(&hp)
            );
            self.contacts[idx].pinned_genesis = new_genesis;
            self.contacts[idx].identity_superseded = false;
            if let Some(storage) = self.storage.as_ref().cloned() {
                if let Err(e) = crate::storage::contacts::save_contact(&self.contacts[idx], &storage)
                {
                    crate::logf!("SUCCESSION: persist migrated pin failed: {}", e);
                }
            }
            // Re-fold the new chain now that its genesis is the pinned one — the fold that arrives adopts the re-founded member set.
            self.spawn_contact_fleet_refresh(vec![hp]);
            needs_redraw = true;
        }

        // Roster-push completion edge: release the in-flight slot; if any push edge fired mid-flight, run the ONE coalesced follow-up now (it re-snapshots the roster, so it carries everything that landed meanwhile).
        if matches!(
            self.roster_push_rx.as_ref().map(|rx| rx.try_recv()),
            Some(Ok(())) | Some(Err(std::sync::mpsc::TryRecvError::Disconnected))
        ) {
            self.roster_push_rx = None;
            if std::mem::take(&mut self.roster_push_queued) {
                self.spawn_roster_push();
            }
        }

        // The key-adoption edge: a parked pull re-fires the moment the cached fleet key differs from the one it failed under. No timer — adoption is the only event that can change the outcome (the 45s refold edge remains the transport-failure backstop).
        if let Some(failed_under) = self.roster_pull_parked_under {
            let current = self.fleet_key_ram.lock().ok().and_then(|g| *g);
            if current != failed_under {
                self.roster_pull_parked_under = None;
                self.needs_initial_roster_pull = true;
                crate::log("FLEET: fleet key landed — re-firing the parked roster pull");
            }
        }

        match self.roster_pull_rx.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(Ok(state))) => {
                self.roster_pull_rx = None;
                self.roster_pull_retries_left = 0;
                self.roster_pull_exhausted = false;
                self.roster_pull_parked_under = None;
                // Settings layers fold in first (global LWW + device newest-copy-wins); a change persists and takes effect on the next read of each key — a sibling's toggle lands here.
                if self.ensure_fleet_settings() {
                    let changed = self
                        .fleet_settings
                        .as_mut()
                        .unwrap()
                        .merge_from(state.global_settings, state.device_settings);
                    // A pulled `fleet.locked` lands here: sweep it onto the sibling rows so every trust gate refuses the locked device from this tick on.
                    self.apply_locked_set();
                    if changed {
                        // Snapshot + OFF-THREAD write: this inline save was one of the ~900ms UI-thread vault writes in the 2026-08-21 field capture, and it fires on every changed fstate merge (the Mac's log showed 37 pulls in one 35-minute window).
                        if let (Some(fs), Some(storage)) =
                            (self.fleet_settings.as_ref(), self.storage.as_ref().cloned())
                        {
                            let snapshot = fs.clone();
                            queue_job(&self.seal_job_tx, move || {
                                if let Err(e) = crate::storage::fleet_settings::save_fleet_settings(
                                    &snapshot, &storage,
                                ) {
                                    crate::logf!("SETTINGS: persist after merge failed: {}", e);
                                }
                            });
                        }
                        self.apply_settings_to_ui();
                        // A sibling's profile edit just landed: refresh the You-page boxes from the merged values (reload-on-next-frame) and republish our pong name in case profile.name changed.
                        self.you_fields_loaded = false;
                        self.publish_profile_name();
                        self.publish_avatar_pin();
                        // A sibling may have changed the avatar (profile.avatar_ts bump rides the same merge) — newest-wins sync pulls the fresh copy; a no-change sync is one cheap wall read.
                        self.spawn_avatar_sync();
                        crate::log("SETTINGS: adopted fleet changes");
                    }
                }
                // Reconcile check BEFORE the merge consumes the pulled roster: do we hold contacts the slot lacks (added while a sibling was the last pusher, or pre-CRDT) or newer LWW stamps? Only then push back — an all-covered pull must NOT push, or the push's fstate event re-pulls every sibling in a ping-pong.
                // Every non-sibling row counts — INCLUDING the self row, which rides the roster like any contact. Excluding it here meant a slot that lost it never got it pushed back, so notes-to-self stayed the one contact a reconcile couldn't restore.
                let slot_missing_ours = self.contacts.iter().any(|c| {
                    if c.is_sibling {
                        return false;
                    }
                    match state
                        .roster
                        .iter()
                        .find(|e| e.handle_proof == c.handle_proof)
                    {
                        None => true,
                        Some(e) => c.roster_updated > e.updated,
                    }
                });
                self.merge_roster_entries(state.roster);
                if slot_missing_ours {
                    crate::log("FLEET: local roster ahead of the slot — pushing back (reconcile)");
                    self.spawn_roster_push();
                }
                needs_redraw = true;
            }
            Some(Ok(Err(ref _e))) => {
                // Pull failed to fetch/decrypt. On a fresh join this is the pairing key still being a pre-rotation generation; the in-flight fan-out key sync writes the current key within ~150ms, so re-arm and retry until the budget runs out (the pull's own round-trip spaces the attempts).
                self.roster_pull_rx = None;
                // THE AEAD BREAKER runs BEFORE any parking (moved 2026-08-21 — it used to live behind retry exhaustion, and the park-on-key-edge retry FROZE the attempts so exhaustion never came: the one device holding a full roster sat parked forever while the whole fleet waited on the re-seal only it could perform). An undecryptable slot is a deadlock, not patience running out: the bytes are sealed under a superseded key, so no re-read helps — a push re-seals from local state.
                // The clobber guard stands unchanged: an aead failure cannot say WHICH side is stale, and a freshly wiped device is the STALE party holding a near-empty roster (macbook field incident, 2026-08-16) — so the breaker fires only when we hold FRIEND rows (non-sibling, non-self) worth sealing; a friendless device parks below and waits for a key edge.
                let mut broke = false;
                if _e.contains("aead") || _e.contains("decrypt") {
                    let our_proof = self.session.as_ref().map(|s| s.handle_proof);
                    let have_friend_state = self
                        .contacts
                        .iter()
                        .any(|c| !c.is_sibling && Some(c.handle_proof) != our_proof);
                    if have_friend_state {
                        crate::log("FLEET: the slot cannot be decrypted under the current key — pushing to re-seal it from local state rather than re-reading bytes nobody can open");
                        self.spawn_roster_push();
                        // Re-pull AFTER our re-seal lands so the merged slot round-trips (the push also fires the worker's fstate event, which re-pulls every sibling).
                        self.needs_initial_roster_pull = true;
                        broke = true;
                    }
                }
                if !broke {
                    if self.roster_pull_retries_left > 0 {
                        self.roster_pull_retries_left -= 1;
                        // Park on the key we failed under; the tick's adoption-edge check re-fires when a DIFFERENT key lands. Re-arming immediately burned the whole budget before any key could arrive.
                        self.roster_pull_parked_under =
                            Some(self.fleet_key_ram.lock().ok().and_then(|g| *g));
                        crate::logf!("FLEET: roster pull failed — parked until a fresh fleet key lands ({} attempt(s) left)", self.roster_pull_retries_left);
                    } else {
                        self.roster_pull_exhausted = true;
                        crate::log("FLEET: roster pull retries exhausted — the 45s refold edge re-arms it (fleet events help sooner when the socket is up)");
                    }
                }
            }
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                self.roster_pull_rx = None; // thread died without sending; drop the dead channel
            }
            _ => {} // still pending, or no pull in flight
        }

        // Every needs_redraw in this function is content (protocol state — presence, roster, ceremony; the blinkey-narrow discipline lives in tick, not here), so convert it to scene_dirty HERE rather than trusting the caller: the Android service tick calls this headless and drops the return, and any content change it applied must still paint on the first visible frame.
        self.scene_dirty |= needs_redraw;
        needs_redraw
    }

    /// Refresh constructor-labeled widgets after a language switch — per-frame set_label sites re-translate on the next paint by themselves; only labels frozen at construction need this.
    pub(super) fn relabel_for_language(&mut self) {
        if let Some(dd) = self.settings_theme_dropdown.as_mut() {
            dd.set_options(vec![tr(Msg::DarkChrome).into_owned(), tr(Msg::LightChrome).into_owned()]);
        }
        if let Some(cb) = self.settings_custodian_check.as_mut() {
            cb.set_label(tr(Msg::CustodianCheckbox));
        }
        if let Some(cb) = self.settings_chime_check.as_mut() {
            cb.set_label(tr(Msg::ChimeNewMessage));
        }
        if let Some(cb) = self.settings_dozenal_check.as_mut() {
            cb.set_label(tr(Msg::Dozenal));
        }
        if let Some(cb) = self.settings_vibrate_msg_check.as_mut() {
            cb.set_label(tr(Msg::VibrateNewMessage));
        }
        if let Some(cb) = self.settings_ring_call_check.as_mut() {
            cb.set_label(tr(Msg::RingIncomingCall));
        }
        if let Some(cb) = self.settings_vibrate_call_check.as_mut() {
            cb.set_label(tr(Msg::VibrateIncomingCall));
        }
        if let Some(cb) = self.settings_presence_check.as_mut() {
            cb.set_label(tr(Msg::PresenceCheckbox));
        }
        if let Some(cb) = self.settings_autoupdate_check.as_mut() {
            #[cfg(target_os = "android")]
            cb.set_label(tr(Msg::AutoUpdateCheck));
            #[cfg(not(target_os = "android"))]
            cb.set_label(tr(Msg::AutoUpdateInstall));
        }
        if let Some(cb) = self.settings_hardlogs_check.as_mut() {
            cb.set_label(tr(Msg::HardLogs));
        }
        if let Some(cb) = self.settings_background_check.as_mut() {
            cb.set_label(tr(Msg::LoadOnStartup));
        }
        if let Some(cb) = self.settings_unattended_check.as_mut() {
            cb.set_label(tr(Msg::UnattendedCheckbox));
        }
        // You-page field labels are baked into the ProfileField structs at build time (field 2026-09-03: "the you page still says maori after I switched back to english") — re-derive them in place. Standard ids relabel thru the catalog; expandable instances (email2, addr3, …) split into base + arabic-suffix count and recompose with fmt_num; custom fields keep the human's verbatim bytes.
        for pf in self.you_fields.iter_mut() {
            if pf.tier == "custom" {
                continue;
            }
            if let Some(label) = profile_field_label(&pf.field_id) {
                pf.label = label.into_owned();
                continue;
            }
            let base = pf.field_id.trim_end_matches(|c: char| c.is_ascii_digit());
            if base.len() < pf.field_id.len() {
                if let (Some(base_label), Ok(n)) = (profile_field_label(base), pf.field_id[base.len()..].parse::<u32>()) {
                    pf.label = format!("{base_label} {}", crate::fmt_num(n));
                }
            }
        }
    }
}
