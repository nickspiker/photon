//! Fleet device management — add-device/join flows, roster sync, fleet key custody, checkpoints, lockout, log submission, and reseeding the derived key sets.

use fgtw::pq::FleetSigner;
use super::*;

/// Where each device keeps the departure requests it denied or heard denied: its own single-writer settings key (`fleet.depart_denied.<device>`, the `react.recent.` shape: the settings layer is LWW per key, so one shared key would drop concurrent denies). Fleet-synced like every setting; the refusal reads the union of every device's list.
const DEPART_DENIED_PREFIX: &str = "fleet.depart_denied.";
/// Photon reports no per-frame send outcome to the Depart machine yet, so a frame with no word back is handed again whenever its device answers a ping (fgtw::flow::deliver).
const DEPART_REPORTS_SENT: bool = false;
/// The same for the Add machine: the grown fan-out with no word back is handed again whenever its device answers a ping.
const ADD_REPORTS_SENT: bool = false;

impl PhotonApp {
    /// The AddDevice screen opens: the Add machine (fgtw::flow::add) is told the page shows, and opens the flow and the radios where this device may add.
    /// Local first (2026-11-10): nearby devices arrive over the LAN, BLE and NFC; fgtw.org's binding requests are read only on the person's edges ("Not nearby? Look through fgtw.org", or typed words that match no device).
    pub(super) fn open_add_device_flow(&mut self) {
        self.add_device_wordcheck_text.clear();
        self.state = AppState::AddDevice;
        if let Some(tb) = self.textbox.as_mut() {
            tb.clear();
        }
        let tb_id = self.textbox.as_ref().map(|t| t.hit_id());
        self.change_focus(tb_id);
        // A beacon heard for the first time wakes the UI thread, whose tick hands it to the machine.
        if let Some(w) = self.event_proxy.clone() {
            crate::network::pairing_beacon::set_waker(Box::new(move || super::wake_ui(&w)));
        }
        if self.add.is_none() {
            crate::log("ADD-DEVICE: no add machine yet (our chain has not folded this session); the flow opens at the first fold");
        }
        self.add_page(true);
    }

    /// Tell the Add machine whether the AddDevice screen is shown, once per change.
    pub(super) fn add_page(&mut self, shown: bool) {
        if self.add_page_shown == shown {
            return;
        }
        self.add_page_shown = shown;
        // The fleet key's presence first: the page opens the flow only where this device holds it.
        self.add_fleet_key_edge();
        self.add_step(fgtw::flow::Event::Page { shown });
    }

    /// Tell the Add machine when this device's hold on the fleet key changes, read from the RAM copy every key writer refreshes (no vault read on this thread).
    pub(super) fn add_fleet_key_edge(&mut self) {
        if self.add.is_none() {
            return;
        }
        let held = self.fleet_key_ram.lock().ok().and_then(|g| *g).is_some();
        if held != self.add_key_held {
            self.add_key_held = held;
            self.add_step(fgtw::flow::Event::FleetKey { held });
        }
    }

    /// The Add machine (fgtw::flow::add), made once per session: from its state in the vault when there is one, else fresh.
    /// False with no session or device key.
    fn add_machine_make(&mut self) -> bool {
        if self.add.is_some() {
            return true;
        }
        let (Some(hp), Some(me), Some(seed)) = (
            self.our_handle_proof(),
            self.device_keypair.as_ref().map(|kp| kp.public.to_bytes()),
            self.session.as_ref().map(|s| s.identity_seed),
        ) else {
            return false;
        };
        let saved = match (self.storage.as_ref(), self.session.as_ref()) {
            (Some(storage), Some(session)) => match storage.read_addr(&crate::storage::vault_key("add_state", &session.vault_seed)) {
                Ok(b) => b,
                Err(e) => {
                    crate::logf!("ADD-DEVICE: the saved add state could not be read ({}); starting fresh", e);
                    None
                }
            },
            _ => None,
        };
        // The handle proof and the identity seed are not in the saved state (roots); they come back from the session.
        self.add = Some(match saved.map(|b| fgtw::flow::add::Add::from_vsf(&b, hp, seed, me, ADD_REPORTS_SENT)) {
            Some(Ok(m)) => m,
            Some(Err(e)) => {
                crate::logf!("ADD-DEVICE: the saved add state did not verify ({}); starting fresh", e);
                fgtw::flow::add::Add::new(hp, seed, me, ADD_REPORTS_SENT)
            }
            None => fgtw::flow::add::Add::new(hp, seed, me, ADD_REPORTS_SENT),
        });
        true
    }

    /// Our chain's fold, as just adopted: the Add machine learns the members, the locked devices and the locked plates.
    /// The first fold of a session also makes it, tells it whether this device holds the fleet key, resumes an add the saved state holds, and opens it when the AddDevice screen is already up.
    pub(super) fn add_members(&mut self, members: &[[u8; 32]]) {
        let first = self.add.is_none();
        if !self.add_machine_make() {
            return;
        }
        let locked = self.locked_devices();
        self.add_step(fgtw::flow::Event::Members { members, locked: &locked });
        let plates: Vec<[u8; 32]> = self
            .our_handle_proof()
            .and_then(|hp| crate::network::fgtw::fleet::held_chain(&hp))
            .and_then(|c| c.locked_plates().ok())
            .unwrap_or_default();
        self.add_step(fgtw::flow::Event::LockedPlates { plates: &plates });
        if !first {
            self.add_fleet_key_edge();
            return;
        }
        let held = self.fleet_key_cached().is_some();
        self.add_key_held = held;
        self.add_step(fgtw::flow::Event::FleetKey { held });
        if let Some(step) = self.add.as_mut().map(|m| m.resume()) {
            self.add_carry(step.actions);
        }
        if self.add_page_shown {
            self.add_step(fgtw::flow::Event::Page { shown: true });
        }
    }

    /// One event to the Add machine, its view kept for the screen and its deadline for the driver, and its actions carried out.
    pub(super) fn add_step(&mut self, ev: fgtw::flow::Event) {
        let Some(m) = self.add.as_mut() else {
            return;
        };
        let step = m.step(ev, vsf::eagle_time_oscillations());
        self.add_view = Some(m.view());
        self.add_wake = step.wake;
        self.add_carry(step.actions);
    }

    /// Per tick: the Add machine's edges Photon learns by looking (the screen shown or gone by any path, the fleet key's RAM copy, BLE beacons heard since the last tick) and its deadline.
    /// Returns whether anything was stepped.
    pub(super) fn add_tick(&mut self) -> bool {
        let mut stepped = false;
        let shown = matches!(self.state, AppState::AddDevice);
        if shown != self.add_page_shown {
            self.add_page(shown);
            stepped = true;
        }
        if self.add.is_none() {
            return stepped;
        }
        if self.fleet_key_ram.lock().ok().and_then(|g| *g).is_some() != self.add_key_held {
            self.add_fleet_key_edge();
            stepped = true;
        }
        if self.beacon_scan_active {
            // Taken before the read, so a beacon heard during it is handed over again next tick rather than missed.
            let now = std::time::Instant::now();
            let since = self.add_ble_fed;
            for b in crate::network::pairing_beacon::heard() {
                if since.map_or(true, |t| b.last_seen > t) {
                    self.add_step(fgtw::flow::Event::BeaconHeard { id: b.uuid });
                    stepped = true;
                }
            }
            self.add_ble_fed = Some(now);
        }
        if let Some((at, reason)) = self.add_wake {
            if vsf::eagle_time_oscillations() >= at {
                self.add_wake = None;
                self.add_step(fgtw::flow::Event::Wake { reason });
                stepped = true;
            }
        }
        stepped
    }

    /// The Add machine's deadline as an Instant for the driver's wake.
    /// A deadline already past is due now.
    pub(super) fn add_wake_at(&self, now: std::time::Instant) -> Option<std::time::Instant> {
        let (at, _) = self.add_wake?;
        let ahead = at - vsf::eagle_time_oscillations();
        // WHY: Duration has no negative values (from_secs_f64 panics on one), and a deadline the clock already passed is simply due.
        Some(if ahead > 0 { now + std::time::Duration::from_secs_f64(ahead as f64 / vsf::OSCILLATIONS_PER_SECOND as f64) } else { now })
    }

    /// Carry out the Add machine's actions with Photon's transport, vault, radios and screen.
    fn add_carry(&mut self, actions: Vec<fgtw::flow::Action>) {
        use fgtw::flow::{Action, Job, To};
        for a in actions {
            match a {
                Action::Send { to: To::Device(k), frame } => self.send_to_device(frame, k),
                Action::Send { to: To::Addr(addr), .. } => crate::logf!("ADD-DEVICE: a frame for the bare address {} was not sent (devices are reached by key)", addr),
                Action::Seen { device, addr } => match self.lan_heard_addr.iter_mut().find(|(k, _)| *k == device) {
                    Some(e) => e.1 = addr,
                    None => self.lan_heard_addr.push((device, addr)),
                },
                Action::Build { id, frame } => {
                    let tx = self.flow_tx.clone();
                    let wake = self.event_proxy.clone();
                    match self.device_signer() {
                        Some(kp) => {
                            // Signing takes every egg this device holds (SLH-DSA among them), so it runs off the UI thread.
                            std::thread::spawn(move || {
                                let result = fgtw::flow::build(&frame, &kp, vsf::eagle_time_oscillations());
                                crate::log_err!(tx.send(FlowDone::AddBuilt { id, result }), "a signed add frame to the UI thread");
                                if let Some(w) = wake.as_ref() {
                                    super::wake_ui(w);
                                }
                            });
                        }
                        None => {
                            crate::log_err!(tx.send(FlowDone::AddBuilt { id, result: Err(tr(fgtw::text::Msg::NoDeviceKey).into_owned()) }), "an add build failure to the UI thread");
                        }
                    }
                }
                Action::Job(Job::ListRequests) => self.add_list_requests(),
                Action::Job(Job::Bind { req }) => self.add_bind(req),
                Action::Job(Job::Grow { device, members }) => self.add_grow(device, members),
                Action::Job(Job::Approve { .. }) => crate::log("ADD-DEVICE: the add machine asked for a departure approval; ignored"),
                Action::Persist => self.add_persist(),
                Action::PushChain { to } => self.push_own_chain_to(to),
                Action::ChainNews { extra } => {
                    // The extended chain goes out now over the mesh: every sibling (their folds take the newcomer in), the newcomer itself (not a sibling yet), and every contact's devices (their folds take it in before it ever speaks to them, field 2026-11-04).
                    // The seed's copy, if it took, is the mirror.
                    self.fleet_chain_push(&[]);
                    if let Some(device) = extra {
                        self.push_own_chain_to(device);
                    }
                    self.fleet_chain_push_to_contacts();
                    self.fleet_chain_store();
                }
                Action::Radios { ble_scan, nfc_reader } => self.add_radios = (ble_scan, nfc_reader),
                Action::RefreshChain => {
                    if let Some(hp) = self.our_handle_proof() {
                        self.spawn_contact_fleet_refresh(vec![hp]);
                    }
                }
                Action::Note(n) => self.add_note(n),
                Action::NfcServe(_) | Action::RecordDenied { .. } | Action::RecordReleased { .. } | Action::Wipe => {
                    crate::log("ADD-DEVICE: the add machine asked for a departure action; ignored");
                }
            }
        }
    }

    /// Job::ListRequests: read fgtw.org's binding requests for our fleet (member-gated, signature-verified in the client) off this thread.
    fn add_list_requests(&self) {
        let tx = self.flow_tx.clone();
        let wake = self.event_proxy.clone();
        let (Some(hp), Some(kp), Some(seed)) = (self.our_handle_proof(), self.device_keypair.clone(), self.session.as_ref().map(|s| s.identity_seed)) else {
            crate::log_err!(tx.send(FlowDone::AddListed(Err(tr(fgtw::text::Msg::NoDeviceKey).into_owned()))), "a registry read failure to the UI thread");
            return;
        };
        crate::log("ADD-DEVICE: reading fgtw.org's binding requests");
        std::thread::spawn(move || {
            let r = crate::network::fgtw::fleet::bindreq_list(&kp, &hp, &seed);
            if let Err(e) = &r {
                crate::logf!("ADD-DEVICE: the binding request read failed: {}", e);
            }
            crate::log_err!(tx.send(FlowDone::AddListed(r)), "the binding requests to the UI thread");
            if let Some(w) = wake.as_ref() {
                super::wake_ui(w);
            }
        });
    }

    /// Job::Bind: the consent-carrying Add appended to the chain we hold and mirrored to the seed (seed-optional), off this thread; a plate the fleet locked out is refused there.
    fn add_bind(&self, req: crate::network::fgtw::fleet::BindRequest) {
        let tx = self.flow_tx.clone();
        let wake = self.event_proxy.clone();
        let device = req.device_pubkey;
        let (Some(hp), Some(kp)) = (self.our_handle_proof(), self.device_signer()) else {
            crate::log_err!(tx.send(FlowDone::AddBound { device, result: Err(tr(fgtw::text::Msg::NoDeviceKey).into_owned()) }), "a bind failure to the UI thread");
            return;
        };
        crate::logf!("ADD-DEVICE: binding {}", crate::fp(&device));
        std::thread::spawn(move || {
            let result = crate::network::fgtw::fleet::bind_device_held(&kp, &hp, &req).map(|_| ());
            if let Err(e) = &result {
                crate::logf!("ADD-DEVICE: the bind of {} failed: {}", crate::fp(&device), e);
            }
            crate::log_err!(tx.send(FlowDone::AddBound { device, result }), "the bind outcome to the UI thread");
            if let Some(w) = wake.as_ref() {
                super::wake_ui(w);
            }
        });
    }

    /// Job::Grow: seal the fleet key held now to the members the machine names (never a locked one), held first and mirrored to the seed, off this thread.
    fn add_grow(&self, device: [u8; 32], members: Vec<[u8; 32]>) {
        let tx = self.flow_tx.clone();
        let wake = self.event_proxy.clone();
        let (Some(hp), Some(kp), Some(seed)) = (self.our_handle_proof(), self.device_keypair.clone(), self.session.as_ref().map(|s| s.identity_seed)) else {
            crate::log_err!(tx.send(FlowDone::AddGrown { device, result: Err(tr(fgtw::text::Msg::NoDeviceKey).into_owned()) }), "a grow failure to the UI thread");
            return;
        };
        let held = self.fleet_key_ram.lock().ok().and_then(|g| *g);
        // Field 2026-10-17 (Emma): every attempt says what it grew and why it stopped.
        crate::logf!("ADD-DEVICE: growing the fan-out to {} member(s) for {} (fleet key held = {})", members.len(), crate::fp(&device), held.is_some());
        std::thread::spawn(move || {
            let result = match held {
                Some(k) => crate::network::fgtw::fleet::fanout_grow_held(&hp, &kp, &k, &members, &seed).map(|(_, blob)| blob),
                None => Err(tr(fgtw::text::Msg::NoFleetKey).into_owned()),
            };
            match &result {
                Ok(_) => crate::log("ADD-DEVICE: fan-out grown; the new device can open the fleet key"),
                Err(e) => crate::logf!("ADD-DEVICE: the grow FAILED: {}", e),
            }
            crate::log_err!(tx.send(FlowDone::AddGrown { device, result }), "the grow outcome to the UI thread");
            if let Some(w) = wake.as_ref() {
                super::wake_ui(w);
            }
        });
    }

    /// Keep the Add machine's state in the vault, so an add under way survives a restart (a bind that landed still gets its key).
    fn add_persist(&self) {
        let (Some(m), Some(storage), Some(session)) = (self.add.as_ref(), self.storage.as_ref(), self.session.as_ref()) else {
            return;
        };
        match m.to_vsf(vsf::eagle_time_oscillations()) {
            Ok(doc) => {
                if let Err(e) = storage.write_addr(&crate::storage::vault_key("add_state", &session.vault_seed), &doc) {
                    crate::logf!("ADD-DEVICE: the add state was not saved: {}", e);
                }
            }
            Err(e) => crate::logf!("ADD-DEVICE: the add state did not encode: {}", e),
        }
    }

    /// The name a device asking to join goes by: the keyed default under our identity seed, the name its own pairing screen shows.
    fn add_device_name(&self, device: &[u8; 32]) -> String {
        self.session
            .as_ref()
            .map(|s| crate::network::fgtw::fleet::device_name_default(device, &s.identity_seed))
            .unwrap_or_else(|| tr(fgtw::text::Msg::ADevice).into_owned())
    }

    /// The AddDevice screen's status line from the Add machine's view (the pick, an add under way, the words typed so far, or contention), and whether it names a typo (drawn red).
    /// Takes the view and the identity seed rather than `self`, so the render can call it while it holds the chrome.
    pub(super) fn add_status_line(view: Option<&fgtw::flow::add::View>, seed: Option<[u8; 32]>) -> (String, bool) {
        use fgtw::flow::add::WordState;
        use fgtw::text::Msg as M;
        let Some(v) = view else {
            return (tr(M::TypeWords).into_owned(), false);
        };
        if let Some((_, name, _)) = v.selected.as_ref() {
            return (tr(M::SelectedDevice(name)).into_owned(), false);
        }
        if let Some((_, name)) = v.adding.as_ref() {
            return (tr(M::AddingDevice(name)).into_owned(), false);
        }
        match v.words.as_ref() {
            Some(WordState::NoCandidates { typo: Some(w) }) => (tr(M::InvalidWord(w)).into_owned(), true),
            Some(WordState::NoCandidates { typo: None }) => (tr(M::WaitingForDevice).into_owned(), false),
            Some(WordState::NoMatch(w)) => (tr(M::NoMatchingDevice(w)).into_owned(), true),
            Some(WordState::One(k)) => {
                let name = v
                    .rows
                    .iter()
                    .find(|r| r.device == *k)
                    .map(|r| r.name.clone())
                    .or_else(|| seed.map(|s| crate::network::fgtw::fleet::device_name_default(k, &s)))
                    .unwrap_or_else(|| tr(M::ADevice).into_owned());
                (tr(M::MatchingDevice(&name)).into_owned(), false)
            }
            Some(WordState::Many(k)) => (tr(M::MatchingMultiple { count: *k as u64, shown: &crate::fmt_mag(*k as u64) }).into_owned(), false),
            // Contended (docs/lifecycle.md P3): only a device beside this one can be added now, by Bluetooth or NFC.
            _ if v.contended => (tr(M::JoinContended).into_owned(), false),
            Some(WordState::Nothing { asking }) if *asking > 1 => (tr(M::DevicesAskingToJoin { count: *asking as u64, shown: &crate::fmt_mag(*asking as u64) }).into_owned(), false),
            _ => (tr(M::TypeWords).into_owned(), false),
        }
    }

    /// What the Add machine says the person should be told.
    fn add_note(&mut self, n: fgtw::flow::Note) {
        use fgtw::flow::Note;
        match n {
            Note::AddNoFleetKey => {
                crate::log("ADD-DEVICE: this device holds no fleet key, so it cannot add a device (nothing written)");
                self.ready_toast = Some(tr(fgtw::text::Msg::NoFleetKey).into_owned());
            }
            Note::AsksToJoin { device } => {
                crate::logf!("JOIN: device {} ({}) asks to join on this network", crate::fp(&device), self.add_device_name(&device));
                self.ready_toast = Some(tr(fgtw::text::Msg::DeviceAsksToJoin).into_owned());
            }
            Note::Picked { device } => crate::logf!("ADD-DEVICE: picked {}; told, nothing bound until Yes", crate::fp(&device)),
            Note::PickUnreachable { device } => crate::logf!("ADD-DEVICE: the picked device {} could not be reached", crate::fp(&device)),
            Note::AddBusy => {
                let busy = self.add_view.as_ref().and_then(|v| v.adding.clone());
                crate::logf!("ADD-DEVICE: another add is under way ({})", busy.as_ref().map(|(k, _)| crate::fp(k)).unwrap_or_default());
                if let Some((_, name)) = busy {
                    self.ready_toast = Some(tr(fgtw::text::Msg::AddingDevice(&name)).into_owned());
                }
            }
            Note::AddLockedOut { device } => {
                crate::logf!("ADD-DEVICE: refused {}: its plate is one this fleet locked out", crate::fp(&device));
                self.ready_toast = Some(tr(Msg::AddDeviceError("this is a device you locked out; unlock it on the Fleet page first")).into_owned());
            }
            Note::AddOwnedElsewhere { device } => crate::logf!("ADD-DEVICE: refused {}: it belongs to another fleet", crate::fp(&device)),
            Note::Adding { device } => {
                let name = self.add_device_name(&device);
                crate::logf!("ADD-DEVICE: adding {} ({}); the bind and the key release run in the background", crate::fp(&device), name);
                // Owner 2026-11-10: "just go straight back to fleet"; the new row and a toast confirm it.
                if matches!(self.state, AppState::AddDevice) {
                    self.end_add_device_flow();
                    self.refresh_fleet_retired();
                    self.state = AppState::Settings(SettingsPage::Fleet);
                }
                self.ready_toast = Some(tr(fgtw::text::Msg::AddingDevice(&name)).into_owned());
            }
            Note::Added { device } => {
                crate::logf!("ADD-DEVICE: {} is in and holds the fleet key", crate::fp(&device));
                self.refresh_fleet_retired();
                self.ready_toast = Some(tr(fgtw::text::Msg::DeviceAdded).into_owned());
                // Recover the current key and re-seal the roster under it in one ordered pass, so the new device's roster pull decrypts.
                self.spawn_roster_republish();
                // And re-fold our own chain now, so the new device gets its sibling row (the fleet weave) without waiting for the next fleet event.
                if let Some(our_hp) = self.our_handle_proof() {
                    self.spawn_contact_fleet_refresh(vec![our_hp]);
                }
            }
            Note::AddFailed { device, reason } => {
                crate::logf!("ADD-DEVICE: adding {} stopped: {}", crate::fp(&device), reason);
                self.ready_toast = Some(tr(Msg::AddDeviceError(&reason)).into_owned());
            }
            Note::ListFailed { reason } => crate::logf!("ADD-DEVICE: fgtw.org's binding requests could not be read ({}); the rows shown stay", reason),
            _ => crate::log("ADD-DEVICE: the add machine sent a departure note; ignored"),
        }
    }

    /// One frame to a device by key, by the nearest path Photon knows: a sibling by its row's addresses and relays, a device not yet in the fold (a newcomer) at the LAN address it was heard from, else over the relay.
    pub(super) fn send_to_device(&self, frame: Vec<u8>, device: [u8; 32]) {
        if self.contacts.iter().any(|c| c.is_sibling && !c.locked_out && c.device_key() == Some(device)) {
            self.dispatch_frame_to_sibling(frame, Some(device));
            return;
        }
        if let Some(addr) = self.lan_heard_addr.iter().find(|(k, _)| *k == device).map(|(_, a)| *a) {
            self.send_frame_direct(frame, device, addr, None);
            return;
        }
        let Some(kp) = self.device_keypair.clone() else {
            return;
        };
        crate::network::http::runtime().spawn(async move {
            if let Err(e) = crate::network::fgtw::relay::send_via_relay_always(&kp, &device, &frame).await {
                crate::logf!("FLEET: a frame to {} over the relay failed: {}", crate::fp(&device), e);
            }
        });
    }

    /// Stop the NEW-device join thread and clear its display state (words, ready light, handle step).
    pub(super) fn end_join_flow(&mut self) {
        if let Some(stop) = self.add_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.add_join_handle = None;
        self.add_join_rx = None;
        self.add_join_words = None;
        self.add_join_hp = None;
        self.add_join_poke = None;
        self.add_join_bind_req = None;
        self.add_join_sent.clear();
        self.add_join_sponsors.clear();
        if let Ok(mut s) = self.add_join_chain.lock() {
            *s = None;
        }
        if let Ok(mut s) = self.add_join_fanout.lock() {
            *s = None;
        }
    }

    /// A fan-out handed to this device (`fanout_resp`). Joining: parked for the join thread (it holds the ira). Attested: a sibling's grow or rotation — opened with our own ira; a key we did not hold is cached (the rotation reaching us without the seed). Returns whether anything changed.
    pub(super) fn on_fanout(&mut self, hp: [u8; 32], blob: &[u8], sender: [u8; 32]) -> bool {
        if self.session.is_none() && self.add_join_hp == Some(hp) {
            if let Ok(mut slot) = self.add_join_fanout.lock() {
                *slot = Some(blob.to_vec());
            }
            if let Some(poke) = self.add_join_poke.as_ref() {
                crate::log_err!(poke.send(()), "the fan-out poke to the joining thread");
            }
            crate::logf!("JOIN: a fan-out for the fleet we are joining arrived over the mesh ({} B)", blob.len());
            return true;
        }
        let (Some(kp), Some(seed)) = (self.device_keypair.clone(), self.session.as_ref().map(|s| s.identity_seed)) else { return false };
        match crate::network::fgtw::fleet::fanout_open_blob(&hp, blob, &kp, &seed) {
            Ok(Some((revision, k))) => {
                if self.fleet_key_cached() != Some(k) {
                    crate::logf!("FANOUT: revision {} from {} over the mesh opens to a key we did not hold — adopting", revision, crate::fp(&sender));
                    self.fleet_key_store(&k);
                    self.reseed_pong_seal_keys();
                    // A key that arrives while the AddDevice screen shows opens the flow there at once.
                    self.add_fleet_key_edge();
                    return true;
                }
                crate::logf!("FANOUT: revision {} from {} over the mesh — same key, held", revision, crate::fp(&sender));
                false
            }
            Ok(None) => {
                crate::logf!("FANOUT: a fan-out from {} over the mesh holds no wrap for us", crate::fp(&sender));
                false
            }
            Err(e) => {
                crate::logf!("SECURITY: a fan-out from {} over the mesh was refused — {}", crate::fp(&sender), e);
                false
            }
        }
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
                .filter(|m| !m.is_control())
                .map(|m| (m.timestamp, ckpt_leaf(m.timestamp, &m.ident_bytes())))
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

    /// The name a departing sibling goes by on the Fleet page (its fleet name), or "a device".
    pub(super) fn departing_name(&self, pk: &[u8; 32]) -> String {
        // The name every fleet surface gives the device (machine_name: its fleet.name.<pk>, else device_name_default under the identity seed), never a contact row's handle-hash pseudonym (field 2026-11-10: the Pro showed as "JewelryView" here and "ProsperityVisionary" everywhere else).
        self.session
            .as_ref()
            .map(|s| super::machine_name(pk, &s.identity_seed, self.fleet_settings.as_ref()))
            .unwrap_or_else(|| tr(fgtw::text::Msg::ADevice).into_owned())
    }

    /// APPROVER: Approve the request shown (one press on the Fleet page, or an NFC tap of the leaving device): the Depart machine runs the approve publish off this thread.
    pub(super) fn approve_pending_departure(&mut self) {
        self.depart_step(fgtw::flow::Event::User(fgtw::flow::UserOp::Approve));
    }

    /// APPROVER: Deny the request shown: refused here and in the fleet-shared settings, and a signed deny goes to every live member, the leaver among them.
    pub(super) fn deny_pending_departure(&mut self) {
        self.depart_step(fgtw::flow::Event::User(fgtw::flow::UserOp::Deny));
    }

    /// The departure request this person is asked about: (leaving device, intent, salt).
    pub(super) fn depart_asked(&self) -> Option<([u8; 32], u8, [u8; 32])> {
        self.depart.as_ref().and_then(|m| m.asked())
    }

    /// Our own departure request stands.
    pub(super) fn depart_asking(&self) -> bool {
        self.depart.as_ref().is_some_and(|m| m.asking())
    }

    /// The Depart machine (fgtw::flow::depart), made once per session: from its state in the vault when there is one, else fresh. False with no session or device key.
    fn depart_machine_make(&mut self) -> bool {
        if self.depart.is_some() {
            return true;
        }
        let (Some(hp), Some(me)) = (self.our_handle_proof(), self.device_keypair.as_ref().map(|kp| kp.public.to_bytes())) else {
            return false;
        };
        let saved = match (self.storage.as_ref(), self.session.as_ref()) {
            (Some(storage), Some(session)) => match storage.read_addr(&crate::storage::vault_key("depart_state", &session.vault_seed)) {
                Ok(b) => b,
                Err(e) => {
                    crate::logf!("FLEET: the saved departure state could not be read ({}); starting fresh", e);
                    None
                }
            },
            _ => None,
        };
        // The handle proof is not in the saved state (a root); it comes back from the session.
        self.depart = Some(match saved.map(|b| fgtw::flow::depart::Depart::from_vsf(&b, hp, me, DEPART_REPORTS_SENT)) {
            Some(Ok(m)) => m,
            Some(Err(e)) => {
                crate::logf!("FLEET: the saved departure state did not verify ({}); starting fresh", e);
                fgtw::flow::depart::Depart::new(hp, me, DEPART_REPORTS_SENT)
            }
            None => fgtw::flow::depart::Depart::new(hp, me, DEPART_REPORTS_SENT),
        });
        true
    }

    /// Our chain's fold, as just adopted: the Depart machine learns the members (and completes our own departure when we are no longer among them). The first fold of a session also resumes whatever the saved state holds.
    pub(super) fn depart_members(&mut self, members: &[[u8; 32]]) {
        let first = self.depart.is_none();
        if !self.depart_machine_make() {
            return;
        }
        let locked = self.locked_devices();
        self.depart_step(fgtw::flow::Event::Members { members, locked: &locked });
        if first {
            if let Some(step) = self.depart.as_mut().map(|m| m.resume()) {
                self.depart_carry(step.actions);
            }
        }
    }

    /// One event to the Depart machine, and its actions carried out.
    pub(super) fn depart_step(&mut self, ev: fgtw::flow::Event) {
        let Some(m) = self.depart.as_mut() else {
            crate::log("FLEET: no departure machine yet (our chain has not folded this session)");
            return;
        };
        let step = m.step(ev, vsf::eagle_time_oscillations());
        self.depart_carry(step.actions);
    }

    /// Carry out the Depart machine's actions with Photon's transport, vault, settings and screen.
    fn depart_carry(&mut self, actions: Vec<fgtw::flow::Action>) {
        use fgtw::flow::{Action, Job, To};
        for a in actions {
            match a {
                Action::Send { to: To::Device(k), frame } => self.dispatch_frame_to_sibling(frame, Some(k)),
                // Departure frames go to members by key; Photon reaches a member by its sibling row's addresses and the relay.
                Action::Send { to: To::Addr(addr), .. } => crate::logf!("FLEET: a departure frame for the bare address {} was not sent (members are reached by key)", addr),
                // Photon learns a sibling's addresses on its own frame path (the sibling row's race addresses).
                Action::Seen { .. } => {}
                Action::Build { id, frame } => {
                    let tx = self.flow_tx.clone();
                    match self.device_signer() {
                        Some(kp) => {
                            // Signing takes every egg this device holds (SLH-DSA among them), so it runs off the UI thread.
                            std::thread::spawn(move || {
                                let result = fgtw::flow::build(&frame, &kp, vsf::eagle_time_oscillations());
                                crate::log_err!(tx.send(FlowDone::Built { id, result }), "a signed departure frame to the UI thread");
                            });
                        }
                        None => {
                            crate::log_err!(tx.send(FlowDone::Built { id, result: Err("no device key to sign with".to_string()) }), "a departure build failure to the UI thread");
                        }
                    }
                }
                Action::Job(Job::Approve { leaver, t, consent_sig, intent }) => {
                    let tx = self.flow_tx.clone();
                    match (self.our_handle_proof(), self.device_signer()) {
                        (Some(hp), Some(kp)) => {
                            // The countersign and publish (and the release for a hand-off) block on the chain write, so off the UI thread.
                            std::thread::spawn(move || {
                                let result = crate::network::fgtw::fleet::approve_departure(&kp, &hp, &leaver, t, &consent_sig, intent);
                                crate::log_err!(tx.send(FlowDone::Approved { leaver, result }), "a departure approval to the UI thread");
                            });
                        }
                        _ => {
                            crate::log_err!(tx.send(FlowDone::Approved { leaver, result: Err("no session or device key".to_string()) }), "a departure approval failure to the UI thread");
                        }
                    }
                }
                Action::NfcServe(salt) => self.depart_nfc_serve = salt.map(|s| crate::network::pairing_nfc::serve_guard(&s)),
                Action::Persist => self.depart_persist(),
                Action::RecordDenied { leaver, salt, t } => self.record_depart_denied(&leaver, &salt, t),
                Action::RecordReleased { device } => {
                    self.settings_set(&format!("fleet.released.{}", hex::encode(device)), vsf::VsfType::ke(device.to_vec()));
                    self.fleet_retired.retain(|d| d != &device);
                }
                Action::PushChain { to } => self.push_own_chain_to(to),
                Action::RefreshChain => {
                    if let Some(hp) = self.our_handle_proof() {
                        self.spawn_contact_fleet_refresh(vec![hp]);
                    }
                }
                Action::Note(n) => self.depart_note(n),
                // Add-only actions (fgtw::flow::add): the Depart machine never asks for them.
                Action::Job(_) | Action::ChainNews { .. } | Action::Radios { .. } => crate::log("FLEET: the departure machine asked for an add action; ignored"),
                Action::Wipe => {
                    crate::log("SECURITY: departure approved by a sibling; the fold no longer lists us; wiping");
                    self.depart = None;
                    self.depart_nfc_serve = None;
                    self.clean_device_for_reuse();
                    return;
                }
            }
        }
    }

    /// Hand our fleet chain as it stands now to one device: the leaver after an approval (so it learns it is out from the chain, not a poll), a device picked on the AddDevice screen (to check the select against), or a newcomer just bound (not a sibling yet, so reached at its LAN address or over the relay).
    fn push_own_chain_to(&self, to: [u8; 32]) {
        let (Some(hp), Some(kp)) = (self.our_handle_proof(), self.device_keypair.as_ref()) else { return };
        let Some(chain) = crate::network::fgtw::fleet::held_chain(&hp) else { return };
        let Ok(bytes) = chain.to_vsf_bytes() else { return };
        match crate::network::fgtw::protocol::build_fleet_resp_vsf(&hp, &bytes, &kp.public.to_bytes(), kp.secret.as_bytes()) {
            Ok(frame) => self.send_to_device(frame, to),
            Err(e) => crate::logf!("FLEET: fleet_resp build failed: {}", e),
        }
    }

    /// Keep the Depart machine's state in the vault, so a departure under way survives a restart.
    fn depart_persist(&self) {
        let (Some(m), Some(storage), Some(session)) = (self.depart.as_ref(), self.storage.as_ref(), self.session.as_ref()) else {
            return;
        };
        match m.to_vsf(vsf::eagle_time_oscillations()) {
            Ok(doc) => {
                if let Err(e) = storage.write_addr(&crate::storage::vault_key("depart_state", &session.vault_seed), &doc) {
                    crate::logf!("FLEET: the departure state was not saved: {}", e);
                }
            }
            Err(e) => crate::logf!("FLEET: the departure state did not encode: {}", e),
        }
    }

    /// What the Depart machine says the person should be told.
    fn depart_note(&mut self, n: fgtw::flow::Note) {
        use fgtw::flow::Note;
        use fgtw::sponsor::Approved;
        match n {
            Note::LastDevice => {
                crate::log("SECURITY: last member (or our chain not folded yet): the departure request is refused (an identity must live somewhere)");
                self.ready_toast = Some(tr(fgtw::text::Msg::LastDevice).into_owned());
            }
            Note::DepartAsking => {
                crate::log("SECURITY: departure requested: the fleet's devices are asked; this device wipes on completion");
                self.depart_was_denied = false;
                self.ready_toast = Some(tr(fgtw::text::Msg::DepartWaitingLine).into_owned());
            }
            Note::DepartNotSent(e) => {
                crate::logf!("SECURITY: the departure request did not build: {}", e);
                self.ready_toast = Some(tr(Msg::SignOutBuildFailed).into_owned());
            }
            Note::DepartDenied { by } => {
                crate::logf!("SECURITY: our departure request was denied by {}; this device stays", crate::fp(&by));
                self.depart_was_denied = true;
                self.ready_toast = Some(tr(fgtw::text::Msg::DepartDenied).into_owned());
            }
            Note::DepartDone => crate::log("SECURITY: our departure is complete"),
            Note::DepartAsked { leaver } => {
                let name = self.departing_name(&leaver);
                crate::logf!("SECURITY: {} requests removal from the fleet: Approve or Deny on the Fleet page", name);
                if let Some((_, _, salt)) = self.depart_asked() {
                    self.notify_departure_request(&name, &salt);
                }
            }
            Note::DepartDeniedHere { leaver } => {
                let name = self.departing_name(&leaver);
                crate::logf!("FLEET: denied {}'s departure request; the fleet is told", name);
                self.ready_toast = Some(tr(fgtw::text::Msg::DepartDeniedHere(&name)).into_owned());
            }
            Note::DepartDeniedElsewhere { leaver, by } => crate::logf!("FLEET: {}'s departure request was denied on {}; cleared here", self.departing_name(&leaver), crate::fp(&by)),
            Note::DepartSettled { leaver } => crate::logf!("FLEET: {}'s departure was settled on another device; cleared here", self.departing_name(&leaver)),
            Note::DepartWithdrawn { leaver } => crate::logf!("FLEET: {} withdrew its departure request; cleared here", self.departing_name(&leaver)),
            Note::DepartApproved { leaver, outcome } => {
                let name = self.departing_name(&leaver);
                crate::logf!("FLEET: countersigned {}'s departure; the consented Remove is published", name);
                self.ready_toast = Some(
                    match outcome {
                        Approved::Released(Ok(())) => tr(fgtw::text::Msg::DepartComplete(&name)),
                        Approved::Released(Err(e)) => {
                            // The countersign landed; only the release failed. The retired row's Release pill is the retry.
                            crate::logf!("FLEET: the release after the countersign failed ({}); the retired row holds the retry", e);
                            tr(fgtw::text::Msg::DepartReleasePending(&name))
                        }
                        Approved::BrandKept => tr(fgtw::text::Msg::FleetSignedOut(&name)),
                    }
                    .into_owned(),
                );
            }
            Note::DepartApproveFailed { leaver, reason } => {
                crate::logf!("FLEET: the approval of {}'s departure did not publish ({}); the request stays", self.departing_name(&leaver), reason);
                self.ready_toast = Some(tr(Msg::SignOutPublishFailed).into_owned());
            }
            // Add notes (fgtw::flow::add): the Depart machine never sends them.
            _ => crate::log("FLEET: the departure machine sent an add note; ignored"),
        }
    }

    /// Encode a list of denied departure requests as one TYPED VSF field: (ke leaver, hb salt, e6 consent stamp) per request, the `encode_react_recent` discipline.
    fn encode_depart_denied(list: &[([u8; 32], [u8; 32], i64)]) -> Vec<u8> {
        use vsf::schema::section::FieldValue;
        let mut values = Vec::with_capacity(list.len() * 3);
        for (leaver, salt, t) in list {
            values.push(vsf::VsfType::ke(leaver.to_vec()));
            values.push(vsf::VsfType::hb(salt.to_vec()));
            values.push(vsf::VsfType::e(vsf::EtType::e6(*t)));
        }
        FieldValue::new("denied", values).flatten()
    }

    /// Decode a denied-requests blob, by type marker: a ke opens an entry, the hb after it is its salt, the e6 after that closes it. A malformed blob reads as empty.
    fn decode_depart_denied(bytes: &[u8]) -> Vec<([u8; 32], [u8; 32], i64)> {
        let mut ptr = 0usize;
        let Ok(field) = vsf::file_format::VsfField::parse(bytes, &mut ptr) else {
            return Vec::new();
        };
        if field.name != "denied" {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut leaver: Option<[u8; 32]> = None;
        let mut salt: Option<[u8; 32]> = None;
        for v in &field.values {
            match v {
                vsf::VsfType::ke(b) => {
                    leaver = <[u8; 32]>::try_from(b.as_slice()).ok();
                    salt = None;
                }
                vsf::VsfType::hb(b) => salt = <[u8; 32]>::try_from(b.as_slice()).ok(),
                vsf::VsfType::e(vsf::EtType::e6(t)) => {
                    if let (Some(l), Some(s)) = (leaver.take(), salt.take()) {
                        out.push((l, s, *t));
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Every departure request refused anywhere in the fleet: the union of every device's denied list (fleet-synced settings), for the Depart machine.
    pub(super) fn depart_denied_union(&self) -> Vec<([u8; 32], [u8; 32], i64)> {
        let Some(fs) = self.fleet_settings.as_ref() else {
            return Vec::new();
        };
        fs.global
            .iter()
            .filter(|e| !e.tombstone && e.key.starts_with(DEPART_DENIED_PREFIX))
            .filter_map(|e| crate::storage::fleet_settings::as_bytes(&e.value))
            .flat_map(|b| Self::decode_depart_denied(&b))
            .collect()
    }

    /// Remember a denied departure request on THIS device's single-writer key, persisted and pushed at once like every setting.
    fn record_depart_denied(&mut self, leaver: &[u8; 32], salt: &[u8; 32], t: i64) {
        let Some(pk) = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes()) else {
            return;
        };
        let key = format!("{DEPART_DENIED_PREFIX}{}", hex::encode(&pk[..8]));
        let mut list: Vec<([u8; 32], [u8; 32], i64)> = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.effective(&key))
            .and_then(crate::storage::fleet_settings::as_bytes)
            .map(|b| Self::decode_depart_denied(&b))
            .unwrap_or_default();
        if list.iter().any(|(l, s, dt)| l == leaver && s == salt && *dt == t) {
            return;
        }
        list.push((*leaver, *salt, t));
        if !self.settings_set(&key, vsf::VsfType::hR(Self::encode_depart_denied(&list))) {
            crate::logf!("FLEET: the deny of {}'s request {} could not be written to settings", crate::fp(leaver), hex::encode(&salt[..4]));
        }
    }

    /// A departure request from a sibling arrived, checked and not denied: tell the person. The OS notification (where the platform has one) opens the Fleet page with the request's Approve and Deny ready; the toast covers an attended window.
    pub(super) fn notify_departure_request(&mut self, name: &str, salt: &[u8; 32]) {
        let body = tr(fgtw::text::Msg::DepartureApproval(name)).into_owned();
        let title = tr(Msg::FleetTitle).into_owned();
        #[cfg(target_os = "android")]
        crate::platform::jni_android::notify_fleet_request(salt, &title, &body);
        #[cfg(not(target_os = "android"))]
        crate::platform::desktop_notify::notify_fleet_request(salt, &title, &body);
        self.ready_toast = Some(body);
    }

    /// REQUESTER: ask the fleet to let this device go (the Security page's Release). The Depart machine (fgtw::flow) refuses on the last device, has the request signed off this thread, and hands it to every live member until each has it; this device wipes when the fold no longer lists it.
    /// `intent` 1: approving frees the hardware too (every Release asks this).
    pub(super) fn request_fleet_departure(&mut self, intent: u8) {
        if !self.depart_machine_make() {
            crate::log("SECURITY: no session or device key to request a departure with");
            self.ready_toast = Some(tr(Msg::NoIdentityToRemove).into_owned());
            return;
        }
        // The salt names this request (owner, 2026-11-10): 32 fresh random bytes inside the envelope our key signs. A Deny refuses it for good, so asking again is always a new salt.
        let salt: [u8; 32] = {
            use rand::RngCore;
            let mut s = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut s);
            s
        };
        self.depart_step(fgtw::flow::Event::User(fgtw::flow::UserOp::Leave { intent, salt }));
    }

    /// Ship one fleet-scoped frame to every non-locked sibling (direct legs race, relay covers the rest): the ckpt frames' transport, same targeting as the chain_sync push.
    pub(super) fn dispatch_frame_to_siblings(&self, frame: Vec<u8>) {
        self.dispatch_frame_to_sibling(frame, None);
    }

    /// One frame to every live sibling, or (`only`) to that one sibling device alone.
    pub(super) fn dispatch_frame_to_sibling(&self, frame: Vec<u8>, only: Option<[u8; 32]>) {
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
            let Some(sib_key) = sib.device_key() else {
                continue; // a sibling row always carries its key; never fabricate one
            };
            if only.is_some_and(|k| k != sib_key) {
                continue;
            }
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
            crate::log_err!(dispatch.send(crate::network::status::HistorySendRequest {
                peer_addr: primary,
                alt_addr: alt,
                recipient_pubkey: sib_key,
                relay_to,
                vsf_bytes: frame.clone(),
                tag: None,
            }), "the checkpoint frame to the network dispatch");
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
                    self.ckpt_spineless_holds += 1; // u32, one per spineless hold
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
        let (Some(kp), Some(hp), Some(fleet_key), Some(identity_seed)) = (
            self.device_keypair.clone(),
            self.our_handle_proof(),
            self.fleet_key_cached(),
            self.session.as_ref().map(|s| s.identity_seed),
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
                crate::log_err!(tx.send(o), "the checkpoint outcome to the UI thread");
            };
            // The fan-out epoch names WHICH fleet key folds into the derivation — recover the pair together so they can't skew.
            let (fanout_epoch, fk) =
                match fleet::recover_fleet_key_with_epoch(&hp, &kp, &identity_seed) {
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
                    crate::logf!(
                        "CKPT: fleet epoch spine {} — reservoir minted, k={} on the chain",
                        if chain_k == 0 { "born" } else { "SUPERSEDED" },
                        new_k
                    );
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
        // Attest-gate only: we no longer filter by our own hp — tick routes by hp (ours vs a contact's).
        if self.our_handle_proof().is_none() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel::<(&'static str, [u8; 32])>();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.fleet_evt_rx = Some(rx);
        self.fleet_evt_stop = Some(stop.clone());
        let wake = self.event_proxy.clone();
        #[cfg(not(target_os = "ferros"))]
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
                                                "idea" => "idea",
                                                "request" => "request",
                                                _ => continue,
                                            };
                                            if tx.send((k, evt_hp)).is_err() {
                                                return; // app side dropped the receiver — subscription retired
                                            }
                                            if let Some(w) = wake.as_ref() {
                                                super::wake_ui(w);
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
    /// PEERS FIRST, SEED LAST (docs/peers-are-fgtw.md §Phase A; Nick 2026-10-26): a contact whose devices we hold an address for is asked for its chain over the mesh (`fleet_req`, answered by any of its devices with a `fleet_resp` that lands in the same drain); the seed is asked only for a contact with no reachable device, or for one whose earlier ask is still unanswered when a refresh fires again (the standing-ask edge — no wall clock). Our own handle always takes the seed path: `fleet::fetch` serves the held copy when the seed is down, and siblings push extensions to us unasked.
    pub(super) fn spawn_contact_fleet_refresh(&mut self, handle_proofs: Vec<[u8; 32]>) {
        if handle_proofs.is_empty() {
            return;
        }
        let our_hp = self.our_handle_proof();
        let mut seed: Vec<[u8; 32]> = Vec::new();
        for hp in handle_proofs {
            if Some(hp) == our_hp {
                seed.push(hp);
                continue;
            }
            let standing = self.fleet_asked.contains(&hp);
            let asked = self.ask_peers_for_chain(hp);
            if asked && !standing {
                self.fleet_asked.push(hp);
            }
            if !asked || standing {
                seed.push(hp);
            }
        }
        if seed.is_empty() {
            return;
        }
        let handle_proofs = seed;
        let tx = self.contact_members_sender();
        let wake = self.event_proxy.clone();
        std::thread::spawn(move || {
            for hp in handle_proofs {
                // Adoption by chain extension (FGTW-03): the read carries whether it lands against the chain we hold, and the drain gates on that, never on the tip's stamp.
                match crate::network::fgtw::fleet::current_fleet_adopting(&hp) {
                    Ok(Some((read, adopt))) => {
                        if tx.send((hp, read.members, read.tip, read.genesis, true, adopt)).is_err() {
                            return; // app dropped the receiver
                        }
                    }
                    Ok(None) => {
                        // No chain under this slot: the drain reads `existed = false` as the identity having ended (docs/lifecycle.md D3).
                        if tx.send((hp, Vec::new(), 0, [0u8; 32], false, false)).is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        crate::logf!(
                            "FLEET: contact fleet refresh failed for {}: {}",
                            crate::fp(&hp),
                            e
                        );
                        // ERROR IS AN EDGE TOO (edges-not-timers, Nick 2026-09-08): tip_ts = -1 is the fetch-failed sentinel — the drain uses it ONLY to clear the tripwire's pursuit entry so the next stale-claim pong may re-fire (paced by ping cadence, no wall clock). existed = true so it can never read as a vanished chain; eagle times are positive, so -1 collides with nothing.
                        if tx.send((hp, Vec::new(), -1, [0u8; 32], true, false)).is_err() {
                            return;
                        }
                    }
                }
            }
            if let Some(w) = wake.as_ref() {
                super::wake_ui(w);
            }
        });
    }

    /// The channel the fold drain reads (protocol.rs member_updates), created on first use — shared by the seed-fetch thread and the peer-chain adoption.
    pub(super) fn contact_members_sender(&mut self) -> std::sync::mpsc::Sender<([u8; 32], Vec<[u8; 32]>, i64, [u8; 32], bool, bool)> {
        if self.contact_members_tx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel::<([u8; 32], Vec<[u8; 32]>, i64, [u8; 32], bool, bool)>();
            self.contact_members_rx = Some(rx);
            self.contact_members_tx = Some(tx);
        }
        self.contact_members_tx.as_ref().unwrap().clone()
    }

    /// One signed frame to a device at an address, direct (no relay: the mesh path is the point here).
    pub(super) fn send_frame_direct(&self, frame: Vec<u8>, device: [u8; 32], addr: std::net::SocketAddr, alt: Option<std::net::SocketAddr>) {
        let Some(checker) = self.status_checker.as_ref() else { return };
        crate::log_err!(checker.history_dispatch().send(crate::network::status::HistorySendRequest {
            peer_addr: addr,
            alt_addr: alt,
            recipient_pubkey: device,
            relay_to: Vec::new(),
            vsf_bytes: frame,
            tag: None,
        }), "a direct frame to the network dispatch");
    }

    /// The periodic own-chain refold, PEERS FIRST (Nick 2026-11-09): every live sibling is asked for the chain it holds, and each sibling pushes every extension it makes, so a sibling's copy is as new as the fleet's.
    /// fgtw.org is asked only when the previous ask went unanswered: an answer adopts and clears `fleet_asked`, so a standing entry here is a round no sibling answered.
    pub(super) fn refold_own_chain(&mut self, our_hp: [u8; 32]) {
        let standing = self.fleet_asked.contains(&our_hp);
        if !standing && self.ask_siblings_for_chain(our_hp) {
            self.fleet_asked.push(our_hp);
            return;
        }
        if standing {
            crate::log("FLEET: no sibling answered the last ask for our chain — asking fgtw.org");
        }
        self.fleet_asked.retain(|h| *h != our_hp);
        self.spawn_contact_fleet_refresh(vec![our_hp]);
    }

    /// Ask every live sibling for OUR fleet's chain (`fleet_req`), at its proven path or else its best candidate. Returns whether any ask went out.
    /// A group member we never friended: its chain is held by whoever talks to it, so ask our own devices and the devices of our contacts who share a group with it (peers first, Nick 2026-11-09); fgtw.org is asked only when none of them answers (the `fleet_asked` rung).
    fn ask_members_for_chain(&self, hp: [u8; 32]) -> bool {
        let Some(kp) = self.device_keypair.as_ref() else { return false };
        let Ok(frame) = crate::network::fgtw::protocol::build_fleet_req_vsf(&hp, &kp.public.to_bytes(), kp.secret.as_bytes()) else { return false };
        let shared: Vec<[u8; 32]> = self
            .molecule_rosters
            .iter()
            .filter(|(_, r)| r.members.values().any(|m| m.handle_proof == hp))
            .flat_map(|(_, r)| r.members.values().map(|m| m.handle_proof).collect::<Vec<_>>())
            .collect();
        let mut sent = 0usize;
        for c in self.contacts.iter().filter(|c| !c.locked_out && (c.is_sibling || shared.contains(&c.handle_proof))) {
            // Our own devices are reached as the sibling ask reaches them.
            if c.is_sibling {
                if let (Some(dev), Some((addr, alt))) = (c.device_key(), c.race_addrs()) {
                    self.send_frame_direct(frame.clone(), dev, addr, alt);
                    sent += 1;
                }
                continue;
            }
            for ep in c.device_endpoints.iter().filter(|e| !c.refused_devices.contains(&e.pubkey)) {
                let (addr, alt) = match (ep.lan(), ep.public()) {
                    (Some(l), p) => (l, p),
                    (None, Some(p)) => (p, None),
                    (None, None) => continue,
                };
                self.send_frame_direct(frame.clone(), ep.pubkey, addr, alt);
                sent += 1;
            }
        }
        if sent > 0 {
            crate::logf!("MOLECULE: asked {} device(s) of our fleet and our group's friends for member {}'s chain", sent, crate::fp(&hp));
        }
        sent > 0
    }

    fn ask_siblings_for_chain(&self, our_hp: [u8; 32]) -> bool {
        let Some(kp) = self.device_keypair.as_ref() else { return false };
        let Ok(frame) = crate::network::fgtw::protocol::build_fleet_req_vsf(&our_hp, &kp.public.to_bytes(), kp.secret.as_bytes()) else { return false };
        let mut sent = 0usize;
        for c in self.contacts.iter().filter(|c| c.is_sibling && !c.locked_out) {
            let (Some(dev), Some((addr, alt))) = (c.device_key(), c.race_addrs()) else { continue };
            self.send_frame_direct(frame.clone(), dev, addr, alt);
            sent += 1;
        }
        if sent > 0 {
            crate::logf!("FLEET: asked {} sibling(s) for our chain over the mesh", sent);
        }
        sent > 0
    }

    /// Ask the contact holding `hp` for its membership chain: a `fleet_req` to every device of theirs we hold an address for. Returns whether any ask went out.
    fn ask_peers_for_chain(&self, hp: [u8; 32]) -> bool {
        let Some(kp) = self.device_keypair.as_ref() else { return false };
        let Some(c) = self.contacts.iter().find(|c| c.handle_proof == hp && !c.is_sibling) else { return self.ask_members_for_chain(hp) };
        let Ok(frame) = crate::network::fgtw::protocol::build_fleet_req_vsf(&hp, &kp.public.to_bytes(), kp.secret.as_bytes()) else { return false };
        let mut sent = 0usize;
        for ep in c.device_endpoints.iter().filter(|e| !c.refused_devices.contains(&e.pubkey)) {
            let (addr, alt) = match (ep.lan(), ep.public()) {
                (Some(l), p) => (l, p),
                (None, Some(p)) => (p, None),
                (None, None) => continue,
            };
            self.send_frame_direct(frame.clone(), ep.pubkey, addr, alt);
            sent += 1;
        }
        if sent == 0 {
            if let (Some(dev), Some((addr, alt))) = (c.device_key(), c.race_addrs()) {
                self.send_frame_direct(frame, dev, addr, alt);
                sent = 1;
            }
        }
        if sent > 0 {
            crate::logf!("FLEET: asked {} device(s) of {} for its chain over the mesh", sent, c.display_name());
        }
        sent > 0
    }

    /// Answer a `fleet_req`: the chain we hold for `hp`, if any, to the asker where it asked from.
    pub(super) fn answer_fleet_ask(&self, hp: [u8; 32], asker: [u8; 32], addr: std::net::SocketAddr) {
        let Some(kp) = self.device_keypair.as_ref() else { return };
        let Some(chain) = crate::network::fgtw::fleet::held_chain(&hp) else {
            crate::logf!("FLEET: {} asked for {}'s chain — we hold none", crate::fp(&asker), crate::fp(&hp));
            return;
        };
        let Ok(bytes) = chain.to_vsf_bytes() else { return };
        match crate::network::fgtw::protocol::build_fleet_resp_vsf(&hp, &bytes, &kp.public.to_bytes(), kp.secret.as_bytes()) {
            Ok(frame) => {
                crate::logf!("FLEET: serving {}'s chain ({} op(s), {} B) to {} at {}", crate::fp(&hp), chain.ops.len(), bytes.len(), crate::fp(&asker), addr);
                self.send_frame_direct(frame, asker, addr, None);
            }
            Err(e) => crate::logf!("FLEET: fleet_resp build failed: {}", e),
        }
    }

    /// A chain a peer handed us (`fleet_resp`): fold it, hold it, and feed the fold drain exactly as a seed fetch would — the drain's genesis pin and monotonic tip decide adoption. For the fleet this device is JOINING (no session yet) the bytes go to the join thread instead, which checks them against its own identity. Returns whether anything changed.
    pub(super) fn adopt_peer_chain(&mut self, hp: [u8; 32], chain: &[u8], sender: [u8; 32]) -> bool {
        if self.session.is_none() && self.add_join_hp == Some(hp) {
            return self.offer_join_chain(chain);
        }
        match crate::network::fgtw::fleet::adopt_chain_bytes(&hp, chain) {
            Ok((blob, members, tip, genesis)) => {
                self.fleet_asked.retain(|h| *h != hp);
                let ours = self.our_handle_proof() == Some(hp);
                crate::logf!("FLEET: {} chain from {} over the mesh — {} op(s), {} member(s), tip {}", if ours { "OUR" } else { "a contact's" }, crate::fp(&sender), blob.ops.len(), members.len(), tip);
                if ours {
                    self.fleet_chain_store();
                }
                // The chain was held by adopt_chain_bytes if the held-chain rule preferred it; it lands in the drain exactly when it is the one we now hold (FGTW-03: adoption by extension, never by tip time).
                let adopt = crate::network::fgtw::fleet::held_chain(&hp).is_some_and(|h| h.head() == blob.head());
                crate::log_err!(self.contact_members_sender().send((hp, members, tip, genesis, true, adopt)), "the chain's members to the contact-members drain");
                true
            }
            Err(e) => {
                crate::logf!("SECURITY: a chain for {} from {} was refused — {}", crate::fp(&hp), crate::fp(&sender), e);
                false
            }
        }
    }

    /// A chain pushed to THIS device while it is joining: park the bytes for the join thread (which holds the identity roots and verifies genesis + membership) and poke it.
    pub(super) fn offer_join_chain(&mut self, chain: &[u8]) -> bool {
        if let Ok(mut slot) = self.add_join_chain.lock() {
            *slot = Some(chain.to_vec());
        }
        if let Some(poke) = self.add_join_poke.as_ref() {
            crate::log_err!(poke.send(()), "the chain poke to the joining thread");
        }
        crate::logf!("JOIN: a chain for the fleet we are joining arrived over the mesh ({} B)", chain.len());
        true
    }

    /// Hand OUR fleet's chain (as held) to every live sibling, and to the extra (device, address) targets — the device just bound, which is not a sibling contact yet.
    pub(super) fn fleet_chain_push(&self, extra: &[([u8; 32], std::net::SocketAddr)]) {
        let (Some(hp), Some(kp)) = (self.our_handle_proof(), self.device_keypair.as_ref()) else { return };
        let Some(chain) = crate::network::fgtw::fleet::held_chain(&hp) else { return };
        let Ok(bytes) = chain.to_vsf_bytes() else { return };
        let Ok(frame) = crate::network::fgtw::protocol::build_fleet_resp_vsf(&hp, &bytes, &kp.public.to_bytes(), kp.secret.as_bytes()) else { return };
        crate::logf!("FLEET: pushing our chain ({} op(s), {} B) to the siblings and {} other device(s)", chain.ops.len(), bytes.len(), extra.len());
        self.dispatch_frame_to_siblings(frame.clone());
        for (dev, addr) in extra {
            self.send_frame_direct(frame.clone(), *dev, *addr, None);
        }
    }

    /// Hand OUR fleet's chain (as held) to every device of every contact we hold an address for, unsolicited: the contacts' folds of our fleet learn an add or a departure from us, now, rather than from a pong of a device they already know.
    /// Field 2026-11-04 (Brittany's phone after its root conversion): her fleet's only device changed keys, so every contact's fold still named the old key, every frame from the new one was "sender not in contacts", and the one mesh ask for her chain went to the key that no longer existed; nothing on her side ever told them.
    /// The receiver adopts a pushed chain by extension against the fold it holds (FGTW-03), signed by the members it already knows, so the sender's own key need not be known to it yet.
    pub(super) fn fleet_chain_push_to_contacts(&self) {
        let (Some(hp), Some(kp)) = (self.our_handle_proof(), self.device_keypair.as_ref()) else { return };
        let Some(chain) = crate::network::fgtw::fleet::held_chain(&hp) else { return };
        let Ok(bytes) = chain.to_vsf_bytes() else { return };
        let Ok(frame) = crate::network::fgtw::protocol::build_fleet_resp_vsf(&hp, &bytes, &kp.public.to_bytes(), kp.secret.as_bytes()) else { return };
        let mut sent = 0usize;
        for c in self.contacts.iter().filter(|c| !c.is_sibling) {
            let mut this = 0usize;
            for ep in c.device_endpoints.iter().filter(|e| !c.refused_devices.contains(&e.pubkey)) {
                let (addr, alt) = match (ep.lan(), ep.public()) {
                    (Some(l), p) => (l, p),
                    (None, Some(p)) => (p, None),
                    (None, None) => continue,
                };
                self.send_frame_direct(frame.clone(), ep.pubkey, addr, alt);
                this += 1;
            }
            if this == 0 {
                if let (Some(dev), Some((addr, alt))) = (c.device_key(), c.race_addrs()) {
                    self.send_frame_direct(frame.clone(), dev, addr, alt);
                    this = 1;
                }
            }
            sent += this;
        }
        crate::logf!("FLEET: pushing our chain ({} op(s), {} B) to {} contact device(s)", chain.ops.len(), bytes.len(), sent);
    }

    /// CHANGES GO TO THE PEOPLE WHO CARE FIRST (Nick 2026-11-10: "broadcast to all my friends that are online, then to the bootstrap ... they update their friends ... timestamped so we know what's fresh"): when the chain this device holds for an identity gets a new head, it goes out over the mesh now, whatever brought it (our own write, a sibling's push, a friend's, a seed read).
    /// Our own chain: to every sibling and every contact's devices. A contact's chain: to our siblings (they track the same contacts) and to that identity's own devices we can reach, so a device that missed its own fleet's change hears it from a friend.
    /// The head is the freshness: the first head seen this run is the baseline, a head already sent is never sent again, and a receiver that holds it already adopts nothing and forwards nothing, so nothing loops.
    pub(super) fn chain_news(&mut self, hp: [u8; 32]) {
        let Some(chain) = crate::network::fgtw::fleet::held_chain(&hp) else { return };
        let head = chain.head();
        match self.chain_heads_sent.get(&hp).copied() {
            None => {
                self.chain_heads_sent.insert(hp, head);
                return;
            }
            Some(sent) if sent == head => return,
            Some(_) => {
                self.chain_heads_sent.insert(hp, head);
            }
        }
        if self.our_handle_proof() == Some(hp) {
            crate::logf!("FLEET: our chain has a new head ({} op(s)) — to the siblings and every contact now", chain.ops.len());
            self.fleet_chain_push(&[]);
            self.fleet_chain_push_to_contacts();
            return;
        }
        let Some(kp) = self.device_keypair.as_ref() else { return };
        let Ok(bytes) = chain.to_vsf_bytes() else { return };
        let Ok(frame) = crate::network::fgtw::protocol::build_fleet_resp_vsf(&hp, &bytes, &kp.public.to_bytes(), kp.secret.as_bytes()) else { return };
        self.dispatch_frame_to_siblings(frame.clone());
        let mut sent = 0usize;
        for c in self.contacts.iter().filter(|c| c.handle_proof == hp && !c.is_sibling) {
            for ep in c.device_endpoints.iter().filter(|e| !c.refused_devices.contains(&e.pubkey)) {
                let (addr, alt) = match (ep.lan(), ep.public()) {
                    (Some(l), p) => (l, p),
                    (None, Some(p)) => (p, None),
                    (None, None) => continue,
                };
                self.send_frame_direct(frame.clone(), ep.pubkey, addr, alt);
                sent += 1;
            }
        }
        crate::logf!("FLEET: {}'s chain has a new head ({} op(s)) — passed on to our siblings and {} of its own device(s)", crate::fp(&hp), chain.ops.len(), sent);
    }

    /// Persist the chain we hold for OUR fleet to the vault (`fleet_chain`), so a device that cannot reach the seed still serves its fleet's chain after a relaunch.
    pub(super) fn fleet_chain_store(&self) {
        let (Some(storage), Some(session)) = (self.storage.as_ref(), self.session.as_ref()) else { return };
        let Some(chain) = crate::network::fgtw::fleet::held_chain(&session.handle_proof) else { return };
        let Ok(bytes) = chain.to_vsf_bytes() else { return };
        let addr = crate::storage::vault_key("fleet_chain", &session.vault_seed);
        if let Err(e) = storage.write_addr(&addr, &bytes) {
            crate::logf!("FLEET: chain store failed: {}", e);
        }
    }

    /// Load our fleet's chain from the vault into the held set (attest/resume), before the first refresh asks the seed — so the seed being down is not a dark fleet.
    pub(super) fn fleet_chain_load(&self) {
        let (Some(storage), Some(session)) = (self.storage.as_ref(), self.session.as_ref()) else { return };
        let addr = crate::storage::vault_key("fleet_chain", &session.vault_seed);
        if let Ok(Some(bytes)) = storage.read_addr(&addr) {
            match crate::network::fgtw::fleet::adopt_chain_bytes(&session.handle_proof, &bytes) {
                Ok((blob, members, _, _)) => crate::logf!("FLEET: our chain loaded from the vault — {} op(s), {} member(s)", blob.ops.len(), members.len()),
                Err(e) => crate::logf!("FLEET: the vault's chain copy was refused — {}", e),
            }
        }
    }

    /// Off-thread: a contact's fold arrived with a genesis DIFFERENT from its pinned one — probe for a succession record that traces back to the pin (docs/identity-succession.md). Fetches the (public) successor slot and runs `verify_for_pin` against `pinned_genesis` HERE, off the UI thread — only a record whose continuity egg is signed by a PREDECESSOR-chain member over the exact transition verifies, so a handle-only impostor cannot forge a re-pin. Posts `(hp, Some(new_genesis))` on success (tick migrates the pin), `(hp, None)` otherwise (tick just clears the in-flight guard so a later refresh re-probes). Results land via `successor_rx`.
    pub(super) fn spawn_successor_check(
        &mut self,
        handle_proof: [u8; 32],
        pinned_genesis: [u8; 32],
    ) {
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
                super::wake_ui(w);
            }
        });
    }

    /// Reconcile OUR OWN folded fleet membership into sibling contacts (the fleet weave). For each member device that isn't us and has no sibling contact yet: create one as `ClutchState::Pending` — the serialized keygen queue picks it up and the full CLUTCH ceremony + weave runs against it exactly like a friend. For each sibling contact whose device fell out of the fold: remove it and delete its state + chains (revocation hygiene — an ex-member must not stay ceremony-eligible). Idempotent; triggered from attest/resume, our-hp `fleet` events, and the Add machine's refold after an add.
    pub(super) fn reconcile_fleet_siblings(&mut self, members: &[[u8; 32]]) {
        // On the alias alone (ferros, PHOTON.md M3) Photon is never a member of the fleet: the fleet's devices are the identity service's, not other Photons, so none becomes a sibling (the 8a's own identity device showed up as a "Pending…" row, 2026-11-13). Photon's siblings there come from the alias's device list.
        if self.photon_keys.as_ref().is_some_and(|k| k.legacy_seed.is_none()) {
            return;
        }
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
                .any(|c| c.is_sibling && c.device_key() == Some(*device))
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

        // The Depart machine (fgtw::flow) learns the fold: when our own request stands and this fold no longer lists us, it completes the local half, the wipe.
        self.depart_members(members);
        if self.session.is_none() {
            return;
        }
        // The Add machine (fgtw::flow) learns it too: who may be added, who the grow leaves out, and whether this device may add at all.
        self.add_members(members);

        // Drop de-folded members (device removed from OUR chain) — AND keyless sibling rows (2026-09-01, the ocean-phone ghost): a sibling contact whose device key is None (or stale-era) can never be pinged, never earns a presence verdict, and the old is_some_and kept it forever — where it wedged the fleet-first keygen gate for every friend ceremony. The fold is the authority; a sibling row it can't vouch for is debris.
        // A sibling row carrying OUR OWN key goes too: the Android root conversion's old device folded the chain that already held the new key and rowed it before exiting, and the carried vault handed that row to the new device, which then relayed history to itself (2026-11-02, "recipient is OUR OWN device — echo refused" ×14).
        let mut removed: Vec<crate::types::Contact> = Vec::new();
        self.contacts.retain(|c| {
            if c.is_sibling && c.device_key().map_or(true, |k| !members.contains(&k) || k == our_device) {
                removed.push(c.clone());
                false
            } else {
                true
            }
        });
        // Ownership is COMPUTED from the fold + presence (era.rs recompute_ceremony_owners, run by the fold-adopt caller once the new fold is stored) — the departed-owner sweep this replaced (Emma's ghost, 2026-09-08: a one-hour device's claim outlived its removal by two weeks) is subsumed: a device not in the fold is never the owner.
        for c in &removed {
            let key = c.device_key().unwrap_or_default();
            crate::logf!(
                "SIBLING: reconciled -1 (device {} {})",
                hex::encode(&key[..4]),
                if key == our_device { "is our own key" } else { "left the fold" }
            );
            changed = true;
            if let Some(storage) = self.storage.as_ref() {
                if let Err(e) =
                    crate::storage::contacts::delete_sibling(&c.device_key().unwrap_or_default(), storage)
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
            // SHRINK ⇒ every friendship re-keys with the old roots woven in (stage 4, decision 1): armed for the epoch AFTER this rotation, so the woven era's chain-sync only ever seals under a key the leaver never held.
            self.arm_heavy_weaves("a device left the fold");
        }

        if changed {
            self.reseed_contact_pubkeys();
            // Freshly-created siblings start address-less; the stalled-contact pulse resolves their devices from the registry (the announce echo that used to address them here is gone — the worker acks without a peer list).
        }

        // A NEWLY-folded sibling has none of the fleet's chain state. Per-lane replication only sends lanes that ADVANCE, so an idle lane already far ahead (a peer device that sent a burst then went quiet) would never reach the new device, leaving it stuck at position 0 for that lane. Clear the per-lane + per-friendship push watermarks so the next drive_chain_replication pass re-pushes EVERY current lane checkpoint to the whole fleet — a one-time full catch-up on the join edge, the deltas resume after. Existing siblings no-op the re-adopt (no greater positions).
        if added_sibling {
            self.lane_pushed_pos.clear();
            self.chain_pushed_osc.clear();
            // The roster-hash dedupe too: a quiet group has no changed lane, so its roster only ever re-pushes when this map forgets it, and a newcomer that never heard the roster never boots the group (2026-11-02, the converted Pixel saw orange and lime, which had changed lanes, and never Kaitiakitanga).
            self.molecule_roster_pushed.clear();
        }
    }

    /// Mint the next fan-out epoch because a sibling pair just became EGGED (Phase A): until its pair secret existed the sibling could not be wrapped, so it held no fleet key. Same rotation the sponsor's confirm runs — the worker's monotonic epoch guard makes concurrent siblings converge, and a `stale` loser simply adopts the winner's key on its next sync.
    /// GROW the fan-out (docs/fleet-key.md): republish wraps for the desired ira set under the SAME held key. Never a mint — the fingerprint does not move, so no sibling has adoption or re-seal work. Fired on the explicit release edges only (green-confirm add, egg completion, unlock) — NEVER from an automatic sweep, so a bound-but-unconfirmed device cannot receive the key before its human's confirm (the two-phase gate).
    pub(super) fn spawn_fleet_key_grow(&self) {
        use crate::network::fgtw::fleet;
        let (Some(hp), Some(device_key), Some(session)) = (
            self.our_handle_proof(),
            self.device_keypair.clone(),
            self.session.as_ref(),
        ) else {
            return;
        };
        let identity_seed = session.identity_seed;
        let busy = self.fleet_heal_busy.clone();
        let held = self.fleet_key_ram.lock().ok().and_then(|g| *g);
        // Snapshot on the UI thread, filter in the closure — a locked device must NOT be wrapped (found 2026-08-15 wiring unlock).
        let locked = self.locked_devices();
        std::thread::spawn(move || {
            // RAII latch: released on EVERY exit, panics included — a manual store(false) here once left a poisoned latch parking key sync for the whole session.
            let Some(_latch) = HealLatch::acquire(&busy) else {
                return; // a heal already holds the latch; its publish covers this too
            };
            // A grow needs the CURRENT key in hand; a device without it has nothing to release — the next keyholder's edge covers the newcomer.
            let Some(held) = held else {
                crate::log("FANOUT: grow skipped — this device does not hold the current key");
                return;
            };
            // Genesis-verified fold: never wrap toward a member set a relay could have swapped.
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
            match fleet::grow_fleet_wraps(&hp, &device_key, &held, &members, &identity_seed) {
                Ok(revision) => crate::logf!(
                    "FANOUT: grew to revision {} — {} ira(s) wrapped, key unchanged",
                    revision,
                    members.len()
                ),
                Err(e) => crate::logf!("FANOUT: grow failed: {}", e),
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
        // "Meaningful" = we hold at least one FRIEND row (non-sibling, non-self): the establish-carry guard pushes near-empty state NEVER (the wiped-device clobber class) but a real roster always.
        let ours_meaningful = {
            let our_hp = self.our_handle_proof();
            self.contacts
                .iter()
                .any(|c| !c.is_sibling && Some(c.handle_proof) != our_hp)
        };
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
            // Fail toward no-mint: any fetch/verify miss here just falls thru to the plain recover path.
            let heal_members = (|| {
                let members = fleet::current_members_verified(&hp).ok()?;
                let desired: Vec<[u8; 32]> = members
                    .into_iter()
                    .filter(|m| !locked.contains(m))
                    .collect();
                let (_, _, _, wraps) = fleet::fetch_fanout(&hp).ok().flatten()?;
                fleet::fanout_needs_rotation(wraps.len(), desired.len()).then_some(desired)
            })();
            if let Some(members) = heal_members {
                // RAII latch — released on every exit including a panic mid-mint (the old manual store left it poisoned).
                let Some(_latch) = HealLatch::acquire(&busy) else {
                    return;
                };
                // THE ATOMIC SHRINK DUTY (docs/fleet-key.md): preserve the slot BEFORE the mint makes it unopenable. A racing sibling's re-push can already have re-sealed it (AEAD miss → default) — then our push carries `ours` alone, and whatever the slot uniquely held rides back on that sibling's next write (CRDT union, no tombstone loss).
                let old_key = storage
                    .read_addr(&addr)
                    .ok()
                    .flatten()
                    .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok());
                let preserved = old_key
                    .and_then(|k| fleet::pull_fstate(&hp, &k).ok().flatten())
                    .unwrap_or_default();
                match fleet::mint_fleet_key(&hp, &device_key, &members, &identity_seed) {
                    Ok((revision, new_key)) => {
                        if let Err(e) = storage.write_addr(&addr, &new_key) {
                            crate::logf!("FLEET: minted key cache failed: {}", e);
                        }
                        if let Ok(mut g) = fkc.lock() {
                            *g = Some(new_key);
                        }
                        let merged = fgtw::fstate::merge_fstate(preserved, ours);
                        match fleet::push_fstate(&hp, &device_key, &new_key, &merged) {
                            Ok(()) => crate::logf!("FLEET: shrink heal — minted at revision {} ({} ira(s) wrapped), fstate re-sealed", revision, members.len()),
                            Err(e) => crate::logf!("FLEET: shrink heal — minted at revision {} but the re-seal push failed ({}); the next roster/settings write re-seals", revision, e),
                        }
                        crate::log_err!(rotated_tx.send(()), "the fleet rotation edge to the UI thread");
                        if let Some(w) = wake.as_ref() {
                            super::wake_ui(w);
                        }
                    }
                    Err(e) => {
                        // Lost the race (or the write failed): a sibling's mint is the live key — adopt it.
                        crate::logf!(
                            "FLEET: shrink heal — mint not ours ({}); adopting the current fan-out",
                            e
                        );
                        if let Ok(Some(k)) =
                            fleet::recover_fleet_key(&hp, &device_key, &identity_seed)
                        {
                            crate::log_err!(storage.write_addr(&addr, &k), "the recovered fleet key write");
                            if let Ok(mut g) = fkc.lock() {
                                *g = Some(k);
                            }
                        }
                    }
                }
                return;
            }
            // A fan-out whose wrap COUNT matches the fold can still hold no wrap for THIS device: the root conversion sponsored the new key into the chain and departed the old one without growing the fan-out (every phone converted on 2026-11-04), so the one wrap there was the dead key's, and a wipe stranded the fleet key with the roster, the backup and the profile behind it (Jeff).
            // Holding the key and opening no wrap is the edge, counts aside: grow now, under the key in hand, for the fold minus the locked.
            let held_here = storage
                .read_addr(&addr)
                .ok()
                .flatten()
                .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok());
            if let Some(held) = held_here {
                if matches!(fleet::recover_fleet_key(&hp, &device_key, &identity_seed), Ok(None)) {
                    let desired: Vec<[u8; 32]> = fleet::current_members_verified(&hp)
                        .map(|m| m.into_iter().filter(|m| !locked.contains(m)).collect())
                        .unwrap_or_default();
                    if !desired.is_empty() {
                        match fleet::grow_fleet_wraps(&hp, &device_key, &held, &desired, &identity_seed) {
                            Ok(revision) => crate::logf!("FANOUT: this device holds the key but the fan-out had no wrap for it — grown to revision {} ({} ira(s) wrapped)", revision, desired.len()),
                            Err(e) => crate::logf!("FANOUT: this device holds the key but the fan-out has no wrap for it, and the grow failed: {}", e),
                        }
                    }
                }
            }
            match fleet::recover_or_establish_carrying(
                &hp,
                &device_key,
                &identity_seed,
                &storage,
                &addr,
                ours,
                ours_meaningful,
            ) {
                Ok(Some(k)) => {
                    if let Err(e) = storage.write_addr(&addr, &k) {
                        crate::logf!("FLEET: fleet key cache failed: {}", e);
                    } else {
                        crate::log("FLEET: fleet key synced from fan-out");
                    }
                    if let Ok(mut g) = fkc.lock() {
                        *g = Some(k);
                    }
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
        // The same carry discipline as the key sync: an establish here must never orphan or clobber the slot (see recover_or_establish_carrying).
        let ours_meaningful = entries.iter().any(|e| e.handle_proof != hp);
        let ours_state = fgtw::fstate::FleetState {
            roster: entries.clone(),
            global_settings: live.as_ref().map(|l| l.0.clone()).unwrap_or_default(),
            device_settings: live.as_ref().map(|l| l.1.clone()).unwrap_or_default(),
        };
        std::thread::spawn(move || {
            match fleet::recover_or_establish_carrying(
                &hp,
                &kp,
                &identity_seed,
                &storage,
                &addr,
                ours_state,
                ours_meaningful,
            ) {
                Ok(Some(k)) => {
                    if let Err(e) = storage.write_addr(&addr, &k) {
                        crate::logf!("FLEET: fleet key cache failed: {}", e);
                    }
                    if let Ok(mut g) = fkc.lock() {
                        *g = Some(k);
                    }
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
        }
    }

    /// Build the fleet roster from the live contact list — the syncable subset, minus fleet siblings (infrastructure, not friends — a sibling pid leaking into the roster would merge as a bogus contact on every device).
    pub(super) fn current_roster(&self) -> Vec<crate::network::fgtw::fleet::RosterEntry> {
        use crate::network::fgtw::fleet::RosterEntry;
        // Every boot this device remembers rides every push as a tombstone (manage.rs): a lost one-shot push, or a re-seal that rebuilt the slot without it, heals on the next push instead of letting a sibling's live entry win.
        let tombstones = self
            .booted
            .iter()
            .filter(|b| !self.contacts.iter().any(|c| !c.is_sibling && c.handle_proof == b.handle_proof))
            .map(|b| RosterEntry {
                handle_proof: b.handle_proof,
                handle_hash: b.party_id,
                // A tombstone carries no identity payload (the boot path's shape); the wire slot's blank is the roster's "no key".
                public_identity: [0u8; 32],
                published_name: String::new(),
                avatar_pin: [0u8; 64],
                added: 0,
                updated: b.at,
                tombstone: true,
                ceremony_owner: [0u8; 32],
                woven: false,
                trust_level: 0,
                consent: false,
            });
        self.contacts
            .iter()
            // The SELF row rides the roster too. Excluding it meant notes-to-self was the one contact a wipe could never restore — you added it deliberately and then had to add it again, every time. It is a contact you chose, so it belongs in the thing that remembers your contacts; the keygen gate skips zero-remote conversations, so it never tries to run a ceremony against itself on arrival.
            .filter(|c| !c.is_sibling)
            .map(|c| RosterEntry {
                handle_proof: c.handle_proof,
                handle_hash: c.handle_hash,
                // Wire slot is fixed [u8;32]; zero = "no device key learned" at this ONE boundary, decoded back to None on the read side. RosterEntry itself is on the zero-sentinel follow-up list.
                public_identity: c.device_key().unwrap_or([0u8; 32]),
                published_name: c.published_name.clone(),
                avatar_pin: c.avatar_pin,
                added: c.added,
                updated: c.roster_updated,
                tombstone: false,
                // §4.2 one-ceremony claim rides the entry. Woven carries whichever truth we hold: OUR chain woven (we're the owner) or the owner's roster-adopted completion.
                ceremony_owner: c.ceremony_owner.unwrap_or([0u8; 32]),
                woven: c.chain_woven || c.owner_woven,
                trust_level: crate::storage::cloud::trust_level_to_u8(c.trust_level),
                // They have added us too — whichever device heard their knock says so for the whole fleet, so the ceremony owner arms even when the knock never reached it.
                consent: c.consent_mutual,
            })
            .chain(tombstones)
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
        // Contacts a sibling says have added us too, armed after the loop (it can remove contacts, so positions are resolved again by proof).
        let mut consent_from_roster: Vec<[u8; 32]> = Vec::new();
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
                // A sibling's entry dated past our known time is refused — adopted, its clock would outrank every honest edit after it.
                if crate::network::time_base::from_the_future(e.updated) {
                    continue;
                }
                if e.tombstone {
                    // A sibling's boot becomes ours too: this device now remembers it and re-emits it on every push.
                    self.note_booted(e.handle_proof, e.handle_hash, e.updated);
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
                        self.set_active_conversation(None);
                        self.contact_boot_armed = false;
                        if matches!(
                            self.state,
                            AppState::Conversation | AppState::ConversationPanel(_)
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
                // The friend's chosen name and pin ride the roster (PRST4), so a fresh sibling shows real names without waiting for each friend's pong; they fill only a field holding no owner-signed record (Contact::fill_from_roster).
                c.fill_from_roster(&e.published_name, &e.avatar_pin);
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
                if e.consent && !c.consent_mutual {
                    consent_from_roster.push(e.handle_proof);
                }
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
                // Removal of a contact we never held: nothing to drop, but remember the boot, so a stale copy arriving later (cloud backup, a lagging sibling) cannot mint it here.
                if !crate::network::time_base::from_the_future(e.updated) {
                    self.note_booted(e.handle_proof, e.handle_hash, e.updated);
                }
                continue;
            }
            // A contact we booted stays gone unless this entry is a deliberate re-add stamped AFTER the boot.
            if self.booted_refuses(&e.handle_proof, e.updated) {
                crate::logf!("BOOT: roster entry for booted contact {} refused (stamped before the boot)", crate::fp(&e.handle_proof));
                continue;
            }
            if self.booted.iter().any(|b| b.handle_proof == e.handle_proof) && !crate::network::time_base::from_the_future(e.updated) {
                self.forget_booted(&e.handle_proof);
            }
            // SELF-GUARD: never mint a NEW row bearing OUR handle_proof under a foreign key — that is the self-contact stub (an empty duplicate whose keygen queue CLUTCHes at our own fleet; the census caught two of them beside the legit self row, 2026-08-11). The true notes-to-self row keys on our identity pid and merges thru the position() match above; anything else claiming our proof is stale roster debris.
            if let (Some(our_proof), Some(our_pid)) = (self.session.as_ref().map(|s| s.handle_proof), self.own_party_id())
            {
                // OLD-SELF IMPORT — MIGRATION-EXPIRES: 2027-02-01 (Nick 2026-10-27: "that import of old will be english only and version out in 12 versions, no reason to keep old non atoms around in the code"); grep the tag to find every piece.
                // Our own identity is never a contact: under a foreign key it is self-stub debris, under our own the retired self row (the conversation with only us in it is an atom now — an older device still pushes it).
                if e.handle_proof == our_proof {
                    if e.handle_hash != our_pid {
                        crate::logf!("FLEET: roster entry claims OUR identity under a foreign key ({}…) — refused (self-stub debris)", hex::encode(&e.handle_hash[..4]));
                    }
                    continue;
                }
            }
            // The roster wire slot's zero means "no device key learned" — decode it back to honest absence, never let it live on as a pingable key.
            let device_pubkey = (e.public_identity != [0u8; 32])
                .then(|| crate::types::DevicePubkey::from_bytes(e.public_identity));
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
            // Whether they have added us comes with the entry: a contact another device added and is still waiting on stays waiting here, never a default "mutual" that hides the wait (2026-10-29).
            contact.consent_mutual = e.consent;
            self.contacts.push(contact);
            added += 1;
        }
        // Another of our devices heard their knock: they have added us, and the ceremony arms here exactly as if the knock had reached this device.
        for hp in consent_from_roster {
            if let Some(ci) = self.contacts.iter().position(|c| !c.is_sibling && c.handle_proof == hp) {
                self.consent_confirmed(ci, "a sibling heard their knock");
            }
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
            // A replicated entry may carry another build's owner view; ours is computed, and this edge restores it.
            self.recompute_ceremony_owners("roster adopt");
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

    /// Merge a fleet state into ours, wherever it came from: `from` names the sibling that pushed it (`fstate_push`, the day-to-day path), `None` the fgtw.org slot (read only when this device holds nothing, the recovery path).
    /// Settings fold first (global newest-wins, device newest-copy-wins), then the roster by its CRDT; when ours holds what the incoming state lacks, the sender gets ours back (a sibling directly, the slot by a backup push).
    pub(super) fn adopt_fleet_state(&mut self, state: fgtw::fstate::FleetState, from: Option<[u8; 32]>) -> bool {
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
                // A sibling's saved draft rides the same slot: newer ones replace ours (and the open compose bar).
                self.merge_remote_drafts();
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
            match from {
                // A sibling's state lacks what we hold: answer that sibling with ours, device to device, and the two converge.
                Some(sib) => {
                    crate::logf!("FLEET: our state is ahead of sibling {}'s — answering it with ours", crate::fp(&sib));
                    self.fleet_state_push(Some(sib));
                }
                None => {
                    crate::log("FLEET: local roster ahead of the slot — pushing back (reconcile)");
                    self.spawn_roster_push();
                }
            }
        }
        true
    }

    /// Hand our fleet's shared state (roster and settings), sealed under the fleet key, straight to our siblings — every live one, or `only` that one (Nick 2026-11-07: fleet state lives on the fleet; fgtw.org keeps a backup).
    /// Fired on every local change beside the backup write, and to a sibling the moment it comes online, so a device that was away catches up from whichever of ours is around.
    pub(super) fn fleet_state_push(&self, only: Option<[u8; 32]>) {
        let (Some(hp), Some(kp), Some(key)) = (self.our_handle_proof(), self.device_keypair.as_ref(), self.fleet_key_cached()) else {
            return;
        };
        let Some(fs) = self.fleet_settings.as_ref() else {
            return;
        };
        let state = fgtw::fstate::FleetState { roster: self.current_roster(), global_settings: fs.global.clone(), device_settings: fs.devices.clone() };
        let sealed = match crate::network::fgtw::fleet::seal_fstate(&state, &key) {
            Ok(s) => s,
            Err(e) => {
                crate::logf!("FLEET: sealing our state for the siblings failed: {}", e);
                return;
            }
        };
        let Ok(frame) = crate::network::fgtw::protocol::build_fstate_push_vsf(&hp, &sealed, kp.secret.as_bytes()) else {
            return;
        };
        match only {
            Some(sib) => crate::logf!("FLEET: our state ({} B, {} roster entr(ies)) to sibling {}", sealed.len(), state.roster.len(), crate::fp(&sib)),
            None => crate::logf!("FLEET: our state ({} B, {} roster entr(ies)) to every live sibling", sealed.len(), state.roster.len()),
        }
        self.dispatch_frame_to_sibling(frame, only);
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
            crate::log_err!(tx.send(result), "the fleet state pull result to the UI thread");
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
        self.apply_settings_to_ui(); // also syncs the render-edge dozenal mirror
        self.publish_profile_name();
        self.publish_avatar_pin();
        true
    }

    /// The fleet device inventory for the Fleet settings page: this device first, then our other devices (sibling contacts). Each entry is `(device_pubkey, is_self, is_online, name)`. Reuses phase-1's sibling reconcile as the live inventory (`current_members(own_hp)` is the authority; reconcile keeps the sibling set == fleet-minus-this-device) — no synchronous network fetch on render. Name = the canonical `device_name_default` (two voca words from the device PUBLIC key + our identity seed), the SAME function the pairing screen uses, so a device shows the same name here, on its own pairing screen, and on every other device in THIS fleet (a handed-off device gets a fresh name in the new owner's fleet).
    /// Rows for the Fleet page: `(pubkey, is_self, online, retired, name, link)`. Current members first (self, then siblings), then the retired inventory — signed out, brand still ours (`fleet_retired`, refreshed on page entry).
    /// `link` is the sibling's LINK STATE in plain words — how far the secure channel to that device has got, plus whether its fan-out pair is egged (Phase A). It is the one place a stuck pairing is visible without reading a log: "securing 5/8" names whose side the ball is on, and "not egged yet" says the device cannot receive the fleet key until its ceremony finishes.
    #[allow(clippy::type_complexity)]
    pub(super) fn fleet_device_rows(
        &mut self,
    ) -> Vec<([u8; 32], bool, bool, bool, String, String, super::ShownTier, String)> {

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
                super::machine_name(&me, &seed, self.fleet_settings.as_ref()),
                String::new(),
                // This device is the vantage point — there is no path to itself to colour or name.
                super::ShownTier::Offline,
                crate::about_string(),
            ));
        }
        // The egged probe remembered (the Fleet page gathers rows per FRAME; a vault read per sibling per frame is a librarian round trip for an answer that only changes at ceremony completion, which clears its entry).
        let mut egged_cache = std::mem::take(&mut self.egged_cache);
        for c in self.contacts.iter().filter(|c| c.is_sibling) {
            let Some(pk) = c.device_key() else {
                continue;
            };
            // Egged = a completed CLUTCH minted this pair's fan-out secret; until then the device gets no wrap and holds no fleet key.
            let egged = *egged_cache.entry(pk).or_insert_with(|| {
                matches!((ours, self.storage.as_ref()), (Some(me), Some(st))
                    if crate::storage::fanout_pairs::load(&me, &pk, st).is_some())
            });
            // One-line row: authored line returns in the ladder/status strings flatten to the middot here.
            let detail = c.clutch_status_detail().replace('\n', " \u{00b7} ");
            let link = if !egged {
                tr(Msg::NotEggedYet(&detail)).into_owned()
            } else {
                detail
            };
            rows.push((
                pk,
                false,
                c.is_online,
                false,
                super::machine_name(&pk, &seed, self.fleet_settings.as_ref()),
                link,
                // A sibling is another physical device — always a remote participant.
                super::path_tier_shown(c, true),
                c.device_about.clone().unwrap_or_default(),
            ));
        }
        self.egged_cache = egged_cache;
        // The page's verdicts, logged on change: which row, what it says, and the presence inputs behind it — so a "shows offline" report can be read against the model that fed it.
        let sig: Vec<([u8; 32], bool, String)> = rows.iter().map(|(pk, _, online, _, _, _, tier, _)| (*pk, *online, super::tier_label(*tier).map(|c| c.into_owned()).unwrap_or_else(|| "offline".into()))).collect();
        if sig != self.fleet_rows_logged {
            for (pk, is_self, online, retired, name, _, tier, _) in &rows {
                let c = self.contacts.iter().find(|c| c.is_sibling && c.device_key() == Some(*pk));
                crate::logf!(
                    "FLEET PAGE: {} \"{}\" — {}{}{} tier {:?}{}",
                    crate::fp(pk),
                    name,
                    if *online { "online" } else { "offline" },
                    if *is_self { " (this device)" } else { "" },
                    if *retired { " (retired)" } else { "" },
                    super::tier_label(*tier).map(|c| c.into_owned()).unwrap_or_else(|| "none".into()),
                    c.map(|c| format!(" — row is_online {} clutch {:?} validated {:?} endpoints {:?}", c.is_online, c.clutch_state, c.validated_path.map(|(a, _)| a), c.device_endpoints.iter().map(|e| (crate::fp(&e.pubkey), e.online)).collect::<Vec<_>>())).unwrap_or_default()
                );
            }
            self.fleet_rows_logged = sig;
        }
        for pk in &self.fleet_retired {
            rows.push((
                *pk,
                false,
                false,
                true,
                super::machine_name(pk, &seed, self.fleet_settings.as_ref()),
                String::new(),
                // Retired: gone from the fleet, so no live path to colour or name.
                super::ShownTier::Offline,
                String::new(),
            ));
        }
        rows
    }

    /// Refresh the Fleet page's retired rows: chain history minus current members minus the `fleet.released` set (brands the owner already freed). Synchronous fetch, page-entry only — same style as the last-member gate. A fetch failure just hides the rows this visit (release is idempotent; nothing is lost).
    pub(super) fn refresh_fleet_retired(&mut self) {
        // A key move leaves the name on the old key; the hardware's plate carries it to the new one (carry_machine_names_by_plate).
        self.carry_machine_names_by_plate();
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
    /// What this device signs fleet writes with — the full bundle when it has one, else the bare keypair. `None` before the keypair exists.
    pub(super) fn device_signer(&self) -> Option<crate::network::fgtw::fleet::DeviceSigner> {
        use crate::network::fgtw::fleet::DeviceSigner;
        self.signing_bundle
            .clone()
            .map(DeviceSigner::Pq)
            .or_else(|| self.device_keypair.clone().map(DeviceSigner::Ed))
    }

    /// Publish this device's key bundle off-thread, if it has one beyond Ed25519. Fired on every attest, so a device on a build that knows the new schemes declares itself the first time it runs — and when the last member does, the fleet's floor rises on its own. Idempotent: the wrapper skips when the chain already holds this bundle.
    pub(super) fn spawn_declare_device(&self, handle_proof: &[u8; 32]) {
        use crate::network::fgtw::fleet::{self, DeviceSigner};
        let Some(DeviceSigner::Pq(bundle)) = self.device_signer() else { return };
        let hp = *handle_proof;
        std::thread::spawn(move || match fleet::declare_device(&bundle, &hp) {
            Ok(()) => crate::log("FLEET: key bundle declared (or already on the chain)"),
            Err(e) => crate::logf!("FLEET: key-bundle declare failed (will retry next attest): {}", e),
        });
    }

    /// Off-thread: read the fleet's scheme facts off our chain (floor, per-member declared masks, chain-locked set) and post them for the Fleet page. Fired on every own-fold adopt and after every declare, so the page shows a migration the moment the chain does.
    pub(super) fn spawn_scheme_refresh(&mut self, handle_proof: &[u8; 32]) {
        if self.fleet_schemes_rx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel::<super::FleetSchemes>();
            self.fleet_schemes_rx = Some(rx);
            self.fleet_schemes_tx = Some(tx);
        }
        let Some(tx) = self.fleet_schemes_tx.clone() else { return };
        let hp = *handle_proof;
        let wake = self.event_proxy.clone();
        std::thread::spawn(move || {
            let Ok(Some(chain)) = crate::network::fgtw::fleet::fetch(&hp) else { return };
            let Ok((members, floor)) = chain.fold_full() else { return };
            let declared = members.iter().map(|m| (*m, chain.declared_mask(m))).collect();
            let locked = chain.locked_out().unwrap_or_default();
            // Each member's stated hardware kind; unstated members are left out and show as such.
            let kinds = members
                .iter()
                .map(|m| (*m, chain.declared_plate_kind(m)))
                .filter(|(_, k)| *k != fgtw::fleet::plate::UNSTATED)
                .collect();
            let facts = super::FleetSchemes { floor, declared, locked, members: members.len(), kinds };
            if tx.send(facts).is_ok() {
                if let Some(w) = wake.as_ref() {
                    super::wake_ui(w);
                }
            }
        });
    }

    pub(super) fn locked_devices(&self) -> Vec<[u8; 32]> {
        self.fleet_settings
            .as_ref()
            .map(|fs| fs.pubkey_set_union("fleet.locked."))
            .unwrap_or_default()
    }

    pub(super) fn is_locked_device(&self, pk: &[u8; 32]) -> bool {
        self.locked_devices().iter().any(|d| d == pk)
    }

    /// Sweep the fleet-synced locked set onto sibling contact rows (persisted, so refusal survives a relaunch before settings sync). Monotonic: locks only, never unlocks — an unlock is a deliberate future flow, not a merge artifact.
    pub(super) fn apply_locked_set(&mut self) {
        let mut locked = self.locked_devices();
        // A locked device is never the ceremony owner (era.rs): re-point on this edge before the self-lock teardown below.
        self.recompute_ceremony_owners("locked set");
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
        // The forgiveness side of the sweep: an EXPLICIT emptied entry (reinstate_fleet_device here, or a sibling's synced tombstone) clears the row. Absence alone never clears — at boot the settings cache lags and the persisted rows below are the only truth, so only an affirmative tombstone may forgive.
        let unlocked = self.unlocked_tombstones();
        if !unlocked.is_empty() {
            let mut cleared: Vec<usize> = Vec::new();
            for (i, c) in self.contacts.iter_mut().enumerate() {
                if c.is_sibling && c.locked_out && c.device_key().is_some_and(|k| unlocked.contains(&k)) {
                    c.locked_out = false;
                    cleared.push(i);
                }
            }
            if !cleared.is_empty() {
                if let Some(storage) = self.storage.as_ref() {
                    for &i in &cleared {
                        crate::logf!(
                            "FLEET: {} is UNLOCKED — the fleet trusts it again",
                            crate::fp(&self.contacts[i].device_key().unwrap_or_default())
                        );
                        crate::log_err!(crate::storage::contacts::save_contact(&self.contacts[i], storage), "the contact save after an unlock");
                    }
                }
                self.reseed_contact_pubkeys();
            }
        }
        // Unlock lifecycle: ack our own row-clear + GC fully-acknowledged markers (Nick 2026-09-02) — runs AFTER the clearing above so an ack always means "this device has forgiven".
        self.ack_unlock_tombstones();
        // Persisted rows count too: at boot the settings cache may not have loaded yet, but a row flagged in a previous session must keep its refusal from the first tick.
        for c in self
            .contacts
            .iter()
            .filter(|c| c.is_sibling && c.locked_out)
        {
            if let Some(k) = c.device_key() {
                    if !locked.contains(&k) {
                        locked.push(k);
                    }
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
            if c.is_sibling && !c.locked_out && c.device_key().is_some_and(|k| locked.contains(&k)) {
                c.locked_out = true;
                changed_rows.push(i);
            }
        }
        if !changed_rows.is_empty() {
            // REVOKE ⇒ the same weave (a revoked device keeps its lane_root; the woven era is the one thing it cannot follow).
            self.arm_heavy_weaves("a device was revoked");
            if let Some(storage) = self.storage.as_ref() {
                for &i in &changed_rows {
                    crate::logf!(
                        "FLEET: {} is LOCKED OUT — frames refused, key rotating away",
                        crate::fp(&self.contacts[i].device_key().unwrap_or_default())
                    );
                    crate::log_err!(crate::storage::contacts::save_contact(&self.contacts[i], storage), "the contact save after a lockout");
                }
            }
            // Rebuild the answerable ping set so a freshly-locked device's PINGS stop being answered immediately (answerable_pubkeys now drops locked_out contacts) — not just its chain/ceremony frames, which the downstream knows_device gates already refused.
            // Without this the flat set kept the device until the next fold reseed, so it went on exchanging presence.
            self.reseed_contact_pubkeys();
        }
    }

    /// Is there ANOTHER device in this fleet that could reinstate us or countersign our departure? The same population `fleet_device_rows` draws (siblings holding a device key, minus retired and minus revoked), read-only and allocation-light so the Security page can ask it every frame. False = a fleet of one, where Revoke and Release are structurally impossible rather than merely unwise.
    pub(super) fn has_usable_sibling(&self) -> bool {
        let revoked = self.locked_devices();
        self.contacts.iter().any(|c| {
            c.is_sibling
                && !c.locked_out
                && c.device_key().is_some_and(|k| !self.fleet_retired.contains(&k) && !revoked.contains(&k))
        })
    }

    /// SELF-REVOKE (Nick 2026-09-08, the desk case): this device revokes ITSELF — you're shelving it, or handing the room to someone, and you'd rather do it here than dig out another device. Same mechanism as revoking a sibling; the difference is the brick gate, because REINSTATE can only come from another device (a revoked device self-locks at every boot, so it can never forgive itself). With no other live device this would be a one-way trip, so it is refused rather than offered.
    pub(super) fn revoke_this_device(&mut self) {
        let Some(ours) = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes()) else {
            return;
        };
        if !self.has_usable_sibling() {
            crate::log("SECURITY: self-revoke refused — no other device could ever reinstate this one");
            self.ready_toast = Some(tr(Msg::RevokeNeedsAnotherDevice).into_owned());
            return;
        }
        crate::log("SECURITY: revoking THIS device at its owner's request — rotating the fleet key away and going dark");
        self.revoke_device(ours);
    }

    /// Treat-as-stolen: lock a fleet device out WITHOUT touching the membership chain (removal is self-signed only, zero exceptions). The device stays a permanent member; the fleet stops trusting it — the locked set syncs fleet-wide, every trust gate refuses it, and the fleet key rotates so its cached key goes stale.
    pub(super) fn revoke_device(&mut self, pk: [u8; 32]) {
        // PER-KEY write, never read-union-write on one blob: two devices locking DIFFERENT pubkeys concurrently each unioned old+own into the same LWW key and one lock was DROPPED — a stolen device stayed trusted on part of the fleet until someone re-locked. Distinct keys commute under merge_global_settings, so a lock can no longer lose a race.
        if !self.is_locked_device(&pk) {
            self.settings_set(
                &format!("fleet.locked.{}", hex::encode(pk)),
                vsf::VsfType::ke(pk.to_vec()),
            );
        }
        // ORDER IS LOAD-BEARING for a SELF-revoke (the Security page's desk case): apply_locked_set tears down our own session the moment it sees our pubkey in the set, and spawn_fleet_key_sync needs that session to mint the next epoch. Rotate and push FIRST, go dark second — otherwise a device revoking itself would leave its own key live in the current epoch and never reach the worker.
        // Rotation NOW, not at the next sentinel pass: the revoked device holds the current fleet key until an epoch it isn't wrapped into exists.
        self.spawn_fleet_key_sync();
        // Push the lock to the worker so the refusal SURVIVES A WIPE: the local fleet.locked set (and the fleet-key rotation) only bind devices that still hold local state, but a wiped-and-reattested stolen device lost all of that — the worker's device_lock entry is the one authority a wipe can't erase, refusing the device at announce.
        self.spawn_worker_lock_push(pk);
        self.apply_locked_set();
    }

    /// The owner's deliberate reversal of `revoke_device` — fires only inside a handle-confirmed attest (see `pending_unlock`).
    /// Order is tombstone-first: the fleet-synced marker empties BEFORE the worker push, so every sibling's `reconcile_worker_locks` stops re-asserting the lock and `reconcile_worker_unlocks` re-drives the clear until the worker agrees — there is no strandable state in either direction.
    /// Re-admission is a GROW: the compliance rotation mints the next epoch including the freshly-eligible device; no shrink semantics involved.
    pub(super) fn reinstate_fleet_device(&mut self, pk: [u8; 32], name: &str) {
        // Value-level tombstone, honestly typed: u0(false) = "not locked". It never parses as a key, so it drops out of pubkey_set_union, and LWW carries the reversal fleet-wide.
        self.settings_set(
            &format!("fleet.locked.{}", hex::encode(pk)),
            vsf::VsfType::u0(false),
        );
        self.apply_locked_set();
        self.spawn_worker_unlock_push(pk);
        self.spawn_fleet_key_grow();
        self.ready_toast = Some(tr(Msg::DeviceReinstatedToast(name)).into_owned());
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
        let signer = crate::network::fgtw::fleet::signer_for(&kp);
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
            match crate::network::fgtw::fleet::unlock_device_chain(&signer, &hp, &pk) {
                Ok(()) => crate::logf!("FLEET: chain lock cleared for {}", crate::fp(&pk)),
                Err(e) => crate::logf!("FLEET: chain unlock failed for {} ({}) — will re-drive on next attest", crate::fp(&pk), e),
            }
        });
    }

    /// UNLOCK-MARKER LIFECYCLE (Nick 2026-09-02: "once my whole fleet re-rolls keys and increments to device total, clear the marker"): every device that has (a) processed an unlock tombstone — its persisted locked rows cleared — and (b) holds the re-grown fleet key (implied: fstate writes SEAL under the current key, so writing the ack proves possession) writes a per-device ack `fleet.unlockack.<pk>.<device>`. When every CURRENT member (self + un-retired, un-locked sibling rows) has acked, the tombstone has finished its job — no device can regress — and ANY member may garbage-collect the `fleet.locked.<pk>` marker plus the acks via entry-level tombstones. A member that slept thru the cycle blocks GC until it returns, clears its row, and acks — exactly the safety the persist-forever rule bought, now with an exit.
    pub(super) fn ack_unlock_tombstones(&mut self) {
        let tombs = self.unlocked_tombstones();
        if tombs.is_empty() {
            return;
        }
        let Some(our_dev) = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes()) else {
            return;
        };
        let mut members: Vec<[u8; 32]> = vec![our_dev];
        for c in self.contacts.iter().filter(|c| c.is_sibling && !c.locked_out) {
            if let Some(k) = c.device_key() {
                if !members.contains(&k) {
                    members.push(k);
                }
            }
        }
        let now = vsf::eagle_time_oscillations();
        for pk in tombs {
            let pk_hex = hex::encode(pk);
            let our_ack = format!("fleet.unlockack.{}.{}", pk_hex, hex::encode(our_dev));
            let have_ours = self
                .fleet_settings
                .as_ref()
                .and_then(|fs| fs.effective(&our_ack))
                .is_some();
            if !have_ours {
                self.settings_set(&our_ack, vsf::VsfType::u0(true));
            }
            // Count DISTINCT acked devices among current members.
            let prefix = format!("fleet.unlockack.{}.", pk_hex);
            let acked: Vec<[u8; 32]> = self
                .fleet_settings
                .as_ref()
                .map(|fs| {
                    fs.global
                        .iter()
                        .filter(|e| !e.tombstone && e.key.starts_with(&prefix))
                        .filter_map(|e| hex::decode(e.key.strip_prefix(&prefix)?).ok())
                        .filter_map(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
                        .collect()
                })
                .unwrap_or_default();
            if members.iter().all(|m| acked.contains(m)) {
                if let Some(fs) = self.fleet_settings.as_mut() {
                    let mut cleared = fs.tombstone_key(&format!("fleet.locked.{pk_hex}"), now);
                    for dev in &acked {
                        cleared |= fs.tombstone_key(
                            &format!("fleet.unlockack.{}.{}", pk_hex, hex::encode(dev)),
                            now,
                        );
                    }
                    if cleared {
                        crate::logf!(
                            "FLEET: unlock of {} acknowledged by all {} member(s) — markers cleared",
                            crate::fp(&pk),
                            members.len()
                        );
                        self.persist_and_push_settings();
                    }
                }
            }
        }
    }

    /// Emptied per-key lock entries — devices the fleet has UNLOCKED (the value-level tombstones `reinstate_fleet_device` writes). The pubkey rides the key's hex suffix since the value is deliberately empty.
    pub(super) fn unlocked_tombstones(&self) -> Vec<[u8; 32]> {
        let Some(fs) = self.fleet_settings.as_ref() else {
            return Vec::new();
        };
        fs.global
            .iter()
            .filter(|e| {
                !e.tombstone && crate::storage::fleet_settings::as_key32(&e.value).is_none()
            })
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
        let signer = crate::network::fgtw::fleet::signer_for(&kp);
        std::thread::spawn(move || {
            match crate::network::fgtw::fleet::lock_device(&kp, &hp, &pk) {
                Ok(()) => crate::logf!(
                    "FLEET: worker lock recorded for {} — brick now survives a wipe",
                    crate::fp(&pk)
                ),
                Err(e) => crate::logf!(
                    "FLEET: worker lock push failed for {} ({}) — will re-drive on next attest",
                    crate::fp(&pk),
                    e
                ),
            }
            // The chain half: what lets the floor stop waiting on a device nobody will ever hold again.
            match crate::network::fgtw::fleet::lock_device_chain(&signer, &hp, &pk) {
                Ok(()) => crate::logf!("FLEET: chain lock recorded for {} — it no longer holds the floor", crate::fp(&pk)),
                Err(e) => crate::logf!("FLEET: chain lock failed for {} ({}) — will re-drive on next attest", crate::fp(&pk), e),
            }
        });
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
            .map(|fs| fs.pubkey_set_union("fleet.released."))
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
            crate::log_err!(tx.send((rows, consumed)), "the log rows to the diagnostics page");
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
            self.ready_toast = Some(tr(Msg::NoLogToSend).into_owned());
            return;
        }
        if self.log_submit_tx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            self.log_submit_rx = Some(rx);
            self.log_submit_tx = Some(tx);
        }
        // hp comes from the SESSION itself, not HandleQuery's worker-populated cache: a sticky-broadcast resume can reach Ready with `last_handle_proof` still unset, which made this gate claim "not signed in" to a user who plainly was (seen live, twice). Signed in ⇒ session ⇒ handle_proof; the cache is only a fallback.
        if let (Some(hp), Some(kp), Some(tx)) = (
            self.our_handle_proof(),
            self.device_keypair.clone(),
            self.log_submit_tx.clone(),
        ) {
            // Immediate press feedback — the read + upload run seconds on a big log, and silence here read as "the button did nothing". Replaced by "Log sent √" / "Send failed" when the worker thread reports.
            self.ready_toast =
                Some(tr(Msg::SendingLog(&crate::unit_size(total as u64, crate::SizeUnit::KiB))).into_owned());
            std::thread::spawn(move || {
                let Some(bytes) = crate::snapshot_log_bytes() else {
                    crate::log_err!(tx.send(Err("log unreadable".to_string())), "the log submit result to the UI thread");
                    return;
                };
                crate::logf!(
                    "DIAG: submitting log ({} bytes) to FGTW (sealed)",
                    bytes.len()
                );
                let r =
                    put_log_blocking(&bytes, &note, &kp, &hp).map_err(|e| format!("{e}"));
                crate::log_err!(tx.send(r), "the log submit result to the UI thread");
            });
            self.log_submit_inflight = true;
        } else {
            self.ready_toast = Some(tr(Msg::CantSendNotSignedIn).into_owned());
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
        self.reseed_molecule_pubkeys();
    }

    /// GROUP admission list (docs/molecules.md §2 / step 5): per group token, every standing member's devices — a friend's answerable set, a never-friended member's MoleculePeer fold. Our own devices never enter (a sibling's frames arrive thru the fleet plane). Rebuilt on every roster, fold and lock edge; the RX worker reads it at the door.
    pub(super) fn reseed_molecule_pubkeys(&self) {
        let ours = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes());
        let mut list: Vec<([u8; 32], Vec<[u8; 32]>)> = Vec::with_capacity(self.molecule_rosters.len());
        for (gid, roster) in &self.molecule_rosters {
            let phase = self.molecule_locals.iter().find(|(g, _)| g == gid).map(|(_, l)| l.phase).unwrap_or_default();
            if matches!(phase, crate::storage::molecule::MoleculePhase::Left) {
                continue;
            }
            let mut devs: Vec<[u8; 32]> = Vec::new();
            for party in roster.standing() {
                if let Some(c) = self.contacts.iter().find(|c| !c.is_sibling && c.handle_hash == party) {
                    devs.extend(c.answerable_pubkeys());
                } else if let Some(p) = self.molecule_peers.iter().find(|p| p.party == party) {
                    devs.extend(p.devices.iter().copied());
                }
            }
            devs.retain(|d| Some(*d) != ours);
            devs.sort_unstable();
            devs.dedup();
            list.push((gid.token(), devs));
        }
        crate::network::status::set_molecule_devices(list);
    }

    /// Rebuild the pairwise pong-seal key map from the same contact walk as `reseed_contact_pubkeys` — one entry per answerable DEVICE pubkey, so the checker can seal a pong to whichever fleet device pinged. Friends: one key per friendship off the static identity DH ([`crate::crypto::clutch::identity_friendship_secret`]), inserted under each of their devices. Siblings — including a self-contact, whose party id IS our own — share the identity seed instead (their party ids aren't curve points, so the DH would come back None anyway) and key per sorted device pair. Runs on the UI thread only: the checker receives finished keys, never the seed.
    pub(super) fn reseed_pong_seal_keys(&self) {
        use zeroize::Zeroize;
        // Same walk publishes the About audience (fleet siblings only — Nick's disclosure ruling 2026-08-31): this is the one place that reads chain-built is_sibling, and it already runs on every membership/spine edge, so the status thread's copy tracks the fold with zero extra edges.
        let sibs: Vec<[u8; 32]> = self
            .contacts
            .iter()
            .filter(|c| c.is_sibling && !c.locked_out)
            .flat_map(|c| c.answerable_pubkeys())
            .collect();
        crate::network::status::set_sibling_devices(sibs);
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
                        None => {
                            crate::network::status::sibling_pong_seal_key(&seed, &our_device, &pk)
                        }
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

    /// The AddDevice screen closes (back, escape, an add starting): the textbox is cleared and the Add machine told, which drops the pick and turns the radios off (an add under way carries on).
    pub(super) fn end_add_device_flow(&mut self) {
        self.join_words_copied = false;
        self.add_device_wordcheck_text.clear();
        if let Some(tb) = self.textbox.as_mut() {
            tb.clear();
        }
        // Drop focus off the words textbox — clearing chars doesn't unfocus it, so without this the (now hidden) widget keeps its blinkey firing, keystrokes still route to it, and Android's soft keyboard stays up over the contact list.
        self.change_focus(None);
        self.add_page(false);
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
        // HANDLES ARE BYTE-PRECISE: the RAW typed string goes to the derivation — no trim, no fold, ever. Non-empty is the ONLY validation; whitespace-only is a valid handle. This was the last surface still trimming (attest never did), which made an edge-whitespace handle attestable but unjoinable.
        if handle.is_empty() {
            return;
        }
        let Some(device_key) = self.device_signer() else {
            self.add_join_status = tr(fgtw::text::Msg::NoDeviceKey).into_owned();
            return;
        };
        // ONE IDENTITY PER DEVICE at the join door (docs/lifecycle.md D2) is gated INSIDE the join thread now, after the proof — the cheap pre-proof compare that used to sit here was the same offline handle oracle as the attest gate (2026-08-23 ticket). The words still show first (they're inert without the bind request), and the gate fires before the request, the NFC serve, or any beacon.
        self.add_join_handle = Some(handle.clone());
        self.add_join_status = tr(fgtw::text::Msg::Preparing).into_owned();
        self.change_focus(None);
        if let Some(tb) = self.textbox.as_mut() {
            tb.clear();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.add_join_rx = Some(rx);
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        self.add_stop = Some(stop.clone());
        // The mesh's two hand-offs to this joiner (a sponsor's chain push, its fan-out push) land in these slots from the UI thread; the thread below owns the identity roots that verify and open them.
        let chain_slot = self.add_join_chain.clone();
        let fanout_slot = self.add_join_fanout.clone();
        if let Ok(mut s) = chain_slot.lock() {
            *s = None;
        }
        if let Ok(mut s) = fanout_slot.lock() {
            *s = None;
        }
        self.add_join_sent.clear();
        self.add_join_sponsors.clear();
        // LAN visibility for the sponsor: the join loop announces this device on the local network (UDP discovery, same frame the fleet uses), so the sponsor's AddDevice screen shows a tappable "nearby" row instead of demanding the words. The words remain the remote/no-LAN fallback.
        let lan_tx = self
            .status_checker
            .as_ref()
            .map(|c| c.lan_broadcast_handle());
        let lan_port = self
            .handle_query
            .as_ref()
            .map(|h| h.port())
            .unwrap_or(crate::PHOTON_PORT);
        // The mailbox opt-in (local first): the join posts to fgtw.org only after the person taps "My devices aren't nearby" (join_mailbox).
        // A sponsor's pick lands here (bind_select), checked by the thread against the fleet's chain before the green.
        let select_slot = self.add_join_select.clone();
        if let Ok(mut s) = select_slot.lock() {
            *s = None;
        }
        self.join_selected_by = None;
        let mailbox = std::sync::Arc::new(AtomicBool::new(false));
        self.join_mailbox = Some(mailbox.clone());
        self.join_mailbox_asked = false;
        std::thread::spawn(move || {
            // Derive the COMPLETE session roots once. identity_seed is microseconds; the ~1s memory-hard proof is reused from the probe when the caller passed it (the add-this-device branch already paid it), else computed here. Joined hands them to the attest worker so it never re-derives.
            let identity_seed = crate::storage::contacts::derive_identity_seed(&handle);
            let me = device_key.keypair().public.to_bytes();
            // SHOW THE WORDS FIRST — they only need identity_seed (microseconds) + the device pubkey, NOT the ~1s memory-hard handle_proof or the radio. Deferring this behind either left the screen on "Preparing…" for the whole proof (and, on Android, behind a blocking BLE-advertise JNI call) — the "stuck on Preparing" report. The words are this device's OWN pubkey masked to the fleet: shoulder-surfing them is inert (nothing binds without the request signature below; the mask makes them noise outside this fleet).
            crate::log_err!(tx.send(JoinUpdate::ShowWords(fleet::masked_device_words(
                &me,
                &identity_seed,
            ))), "the join words to the UI thread");
            // NOW the expensive derivation (reused from the probe when the caller passed it; else the ~1s proof here) — the words are already up, so this cost is invisible.
            let handle_proof = precomputed_proof
                .unwrap_or_else(|| crate::types::Handle::username_to_handle_proof(&handle));
            let session = tohu::SessionIdentity {
                identity_seed,
                vault_seed: identity_seed,
                handle_proof,
            };
            // The join door's DEVICE BUSY verdict (docs/lifecycle.md D2), post-proof so the marker never verifies a cheap guess (2026-08-23 oracle fix). Everything network-visible — the bind request, the NFC secret, the LAN beacon — sits below this gate; the words above are this device's own masked pubkey and bind nothing.
            if crate::storage::device_binding::busy_for(
                &handle_proof,
                &crate::crypto::clutch::identity_party_id(&identity_seed),
            ) {
                crate::log("join: DEVICE BUSY — bound to another identity; refused post-proof");
                crate::log_err!(tx.send(JoinUpdate::Failed(tr(Msg::IdentityOccupied).into_owned())), "the join refusal to the UI thread");
                return;
            }
            let hp = session.handle_proof;
            // NFC instant-add secret: 32 random bytes for THIS join session, served as a dumb tag over HCE (Android) while the ceremony is up. The put publishes its keyed commitment; a sponsor's tap reads S and matches it — proximity IS the selection. One S per session (the commitment re-binds each repost's fresh stamp).
            let nfc_secret: [u8; 32] = {
                use rand::RngCore;
                let mut s = [0u8; 32];
                rand::thread_rng().fill_bytes(&mut s);
                s
            };
            let _nfc_serve = crate::network::pairing_nfc::serve_guard(&nfc_secret);
            // The wake channel: the seed's hub events, AND the UI thread's pokes when a chain or a fan-out for this fleet arrives over the mesh (JoinUpdate::Aimed hands the UI the sender).
            let (wake_tx, wake_rx) = std::sync::mpsc::channel::<()>();
            let poke_tx = wake_tx.clone();
            // The binding request: "I consent to join fleet hp", signed by the device key + co-signed by the identity key — the consent egg the Add op will carry. Built ONCE and sent two ways with the same stamp (docs/pairing-v2.md over the mesh): to the seed's registry when it answers, and straight to every sponsor device heard on the LAN as a `bind_req` frame (the UI thread hands it out on each beacon). A seed that cannot be reached is logged, never fatal — the LAN carries the request; the seed's one-owner verdict (device_owned) still ends the ceremony.
            let mint = |tx: &std::sync::mpsc::Sender<JoinUpdate>| -> Result<fgtw::fleet::BindRequest, String> {
                let req = fleet::make_bind_request(&device_key, &identity_seed, &hp, &nfc_secret);
                let frame = crate::network::fgtw::protocol::build_bind_req_vsf(&req, &hp, device_key.keypair().secret.as_bytes())?;
                crate::log_err!(tx.send(JoinUpdate::Aimed { hp, bind_req: frame, poke: poke_tx.clone() }), "the join aim to the UI thread");
                Ok(req)
            };
            // LOCAL FIRST (Nick 2026-11-10: fgtw.org "a notify upon request"): the request goes to the person's devices nearby (LAN beacon, the direct bind_req, NFC), and fgtw.org's registry is posted to only when the person says their devices aren't nearby (the mailbox flag); until then fgtw.org is not touched at all.
            let posted = std::cell::Cell::new(false);
            let post = |req: &fgtw::fleet::BindRequest| -> Result<(), String> {
                match fleet::bindreq_post(req, &hp) {
                    Ok(()) => crate::log("JOIN: binding request posted to fgtw.org's mailbox, as asked"),
                    Err(e) if e.starts_with("device_owned") => return Err(e),
                    Err(e) => crate::logf!("JOIN: fgtw.org did not take the binding request ({}) — the LAN carries it", e),
                }
                Ok(())
            };
            let mut req = match mint(&tx) {
                Ok(r) => r,
                Err(e) => {
                    crate::log_err!(tx.send(JoinUpdate::Failed(tr(Msg::RequestFailed(&e)).into_owned())), "the join failure to the UI thread");
                    return;
                }
            };
            let offer_et = req.t;
            // Pairing v2 proximity beacon (docs/pairing-v2.md): advertise keyed_hash(device_pk ‖ published eagle_time) within the handle for the whole ceremony — the guard drops on every exit path below, stopping the radio. Re-announced on each repost (below) so the aired id tracks the offer's current stamp.
            let _beacon = crate::network::pairing_beacon::announce_guard(&hp, &me, offer_et);

            // Push subscription: the FGTW hub broadcasts `pair_evt` frames on registry changes ("request") and when the chain extends ("fleet"). Each one for OUR identity pokes `wake_tx`, and the poll loop's sleep below is a `recv_timeout` — so the ceremony reacts the moment the other device acts, with the poll cadence as the guarantee when the socket drops (best-effort accelerator, never load-bearing). The task dies with the socket or the stop flag; no reconnect — the poll still covers everything.
            // Started only with the mailbox (local first): until the person asks through fgtw.org, nothing in this ceremony connects to it.
            let mut hub_wake = Some(wake_tx);
            let subscribe_hub = |wake_tx: std::sync::mpsc::Sender<()>| {
                #[cfg(not(target_os = "ferros"))]
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
                                                    // The poll loop holding wake_rx has ended (its ceremony is over): this subscription has no one left to wake.
                                                    if wake_tx.send(()).is_err() {
                                                        return;
                                                    }
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
                #[cfg(target_os = "ferros")]
                drop(wake_tx);
            };
            // PUSH-DRIVEN, no deadline, no poll cadence. The hub events (request / fleet) wake each check instantly; the ONLY timers are the ones the protocol/transport demand — the binding request's 5-minute freshness (re-post at ~3.5min when no event arrives sooner) and a degraded-transport fallback cadence when the socket is dead. The user standing at the screen is the timeout: the ceremony ends when the bind lands, when they tap the orb (the stop flag), or on a hard network error. The request is WITHDRAWN by this device (its author) on every exit — green, cancel, or wrong-fleet — and lapses by stamp if the thread dies unclean.
            let withdraw = |why: &str| {
                if !posted.get() {
                    return; // nothing was posted to withdraw
                }
                if let Err(e) = fleet::bindreq_withdraw(device_key.keypair(), &hp) {
                    crate::logf!(
                        "JOIN: request withdraw ({}) failed (lapses anyway): {}",
                        why,
                        e
                    );
                }
            };
            // GREEN: this device folds into the fleet's chain. Leaving the words screen is what the far-side human confirms, so it happens NOW; the key follows — a fan-out handed over the mesh opens here at once, else the seed's, else the post-attest sync.
            let green = |tx: &std::sync::mpsc::Sender<JoinUpdate>, how: &str| {
                crate::logf!("JOIN: bound — this device is in the fleet chain ({})", how);
                withdraw("green");
                let pushed = fanout_slot.lock().ok().and_then(|mut s| s.take()).and_then(|blob| fleet::fanout_open_blob(&hp, &blob, device_key.keypair(), &identity_seed).ok().flatten().map(|(_, k)| k));
                let fleet_key = pushed.or_else(|| fleet::recover_fleet_key(&hp, device_key.keypair(), &identity_seed).ok().flatten());
                crate::logf!(
                    "JOIN: fleet key {} — attesting",
                    if fleet_key.is_some() { "recovered" } else { "follows the sponsor's confirm (event-synced)" }
                );
                crate::log_err!(tx.send(JoinUpdate::Joined(fleet_key, session)), "the join success to the UI thread");
            };
            let mut last_repost = std::time::Instant::now();
            let mut known_chain: Option<fleet::MembershipBlob> = None;
            let mut pending_select: Option<[u8; 32]> = None;
            let ceremony_start = std::time::Instant::now();
            // Give-up backstop: a real ceremony binds in seconds-to-a-minute. Past this it's a forgotten or backgrounded screen — and Android keeps this raw thread alive after the Activity pauses (onPause emits no focus event), so without a cap it re-posts the registry every 3.5 min forever (observed: a backgrounded phone re-posting for 1.6h). Generous enough that a live pairing never hits it.
            const MAX_CEREMONY: std::time::Duration = std::time::Duration::from_secs(15 * 60);
            loop {
                if stop.load(Ordering::Relaxed) {
                    withdraw("cancel");
                    return;
                }
                // The person asked through fgtw.org (their devices aren't nearby): post the request there, once, and start listening to its hub.
                if mailbox.load(Ordering::Relaxed) && !posted.get() {
                    if let Err(e) = post(&req) {
                        crate::log_err!(tx.send(JoinUpdate::Failed(tr(Msg::RequestFailed(&e)).into_owned())), "the join failure to the UI thread");
                        return;
                    }
                    posted.set(true);
                    if let Some(w) = hub_wake.take() {
                        subscribe_hub(w);
                    }
                }
                if ceremony_start.elapsed() >= MAX_CEREMONY {
                    crate::log("JOIN: ceremony ran too long without binding — stopping re-posts");
                    withdraw("timeout");
                    crate::log_err!(tx.send(JoinUpdate::Failed(
                        "timed out waiting to be added\nre-enter the handle to try again".into(),
                    )), "the join timeout to the UI thread");
                    return;
                }
                // Announce on the LAN each round (rides the poll cadence): the sponsor's screen lights this device's candidate row "nearby" the moment a frame lands.
                if let Some(tx) = lan_tx.as_ref() {
                    crate::log_err!(tx.send(crate::network::status::LanBroadcastRequest {
                        our_handle_proof: hp,
                        our_port: lan_port,
                        unicast: None,
                    }), "the join LAN announce request");
                }
                // THE MESH FIRST: a chain a sponsor pushed to us. Verified exactly as a seed copy is — it claims this slot, its genesis is this identity's, it folds — and green iff it folds US in. A chain that does not name us yet is held (the sponsor may have pushed before the bind) and we wait on.
                if let Some(bytes) = chain_slot.lock().ok().and_then(|mut s| s.take()) {
                    match fleet::MembershipBlob::from_vsf_bytes(&bytes) {
                        Ok(blob) if blob.genesis_handle_proof() == Some(hp) && blob.genesis_identity_matches(&identity_seed) => match blob.fold() {
                            Ok(members) => {
                                fleet::hold_chain(&blob);
                                if members.contains(&me) {
                                    green(&tx, "over the mesh");
                                    return;
                                }
                                crate::log("JOIN: a chain for our fleet arrived over the mesh — not bound yet");
                                // Kept: a sponsor's pick is checked against it.
                                known_chain = Some(blob.clone());
                            }
                            Err(e) => crate::logf!("SECURITY: a chain pushed to this joiner does not fold ({}) — ignored", format!("{:?}", e)),
                        },
                        Ok(_) => crate::log("SECURITY: a chain pushed to this joiner is not this identity's fleet — ignored"),
                        Err(e) => crate::logf!("JOIN: a chain pushed to this joiner did not parse ({}) — ignored", e),
                    }
                }
                // A sponsor's pick (bind_select): green only when the sender folds as a live member of this identity's chain (the one pushed to us, or one held for this slot whose genesis is this identity's); held until a chain arrives to check it against.
                if let Some(s) = select_slot.lock().ok().and_then(|mut g| g.take()) {
                    pending_select = Some(s);
                }
                if let Some(sender) = pending_select {
                    let chain = known_chain.clone().or_else(|| fleet::held_chain(&hp).filter(|c| c.genesis_handle_proof() == Some(hp) && c.genesis_identity_matches(&identity_seed)));
                    if let Some(c) = chain {
                        pending_select = None;
                        let live = c.fold().ok().zip(c.locked_out().ok()).is_some_and(|(m, l)| m.contains(&sender) && !l.contains(&sender));
                        if live {
                            crate::log_err!(tx.send(JoinUpdate::Selected(fleet::device_name_default(&sender, &identity_seed))), "the green to the UI thread");
                        } else {
                            crate::logf!("SECURITY: a pick from {} is not from a live member of this fleet — ignored", crate::fp(&sender));
                        }
                    }
                }
                // Genesis re-checked on EVERY fetch, not just the probe: a relay that served the real chain once must not be able to swap in a structurally-valid foreign chain mid-ceremony and have this loop adopt its members (the probe-time-only TOCTOU — docs/pairing-v2.md).
                // fgtw.org's copy is read only once the person asked through it; nearby, the sponsor hands this device its chain directly (above).
                match if posted.get() { fleet::current_members_verified(&hp) } else { Ok(Vec::new()) } {
                    Ok(m) if m.contains(&me) => {
                        // In the fleet by the seed's (or the held) copy — the green.
                        green(&tx, "by the seed");
                        return;
                    }
                    Ok(_) => {} // not bound yet — keep the request fresh and wait
                    Err(e) if e.contains("not rooted in this identity") => {
                        // Wrong-fleet is a VERDICT, not a blip: the chain now folds to a genesis that isn't ours, mid-ceremony. Scream and stop — retrying would just poll an imposter chain forever.
                        crate::log(
                            "JOIN: chain genesis no longer matches this identity — aborting",
                        );
                        withdraw("wrong-fleet");
                        crate::log_err!(tx.send(JoinUpdate::Failed(
                            "this handle's fleet is not ours\nthe chain changed hands mid-join"
                                .into(),
                        )), "the join abort to the UI thread");
                        return;
                    }
                    Err(e) => {
                        // A transient fetch failure (laptop sleep/wake, dropped wifi, a server blip, or no seed at all) must NOT kill the ceremony — the user is still standing at the words screen, and a dead thread strands it there forever (observed: macbook stuck on its words after one failed fetch). Log and fall through to the wait below, where a mesh poke (the sponsor's chain push) wakes us at once; the stop flag (orb cancel) is the only exit.
                        crate::logf!("JOIN: membership fetch failed ({}) — the mesh may answer first", e);
                    }
                }
                // Wait for a push, but re-poll membership on a SHORT cadence so the ceremony completes fast even when the hub WebSocket push never arrives (observed on macOS: the bind landed but the new device sat ~3.5 min on the old 210s timeout before its next `current_members` check). The request re-post stays on its protocol cadence (~3.5 min — the registry stamp lapses at 5), tracked separately so the frequent membership polls don't spam re-posts. A hub event still short-circuits instantly.
                const MEMBERSHIP_POLL: std::time::Duration = std::time::Duration::from_secs(8);
                const REPOST_EVERY: std::time::Duration = std::time::Duration::from_secs(210);
                match wake_rx.recv_timeout(crate::jitter_dur(MEMBERSHIP_POLL)) {
                    Ok(()) => {} // hub push — loop re-checks membership immediately
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if last_repost.elapsed() >= REPOST_EVERY {
                            // Restamp the offer (registry + LAN, one stamp), then re-emit the beacon under the NEW eagle_time so the aired id keeps matching the entry the sponsor now reads.
                            if let Ok(r) = mint(&tx) {
                                crate::network::pairing_beacon::reannounce(&hp, &me, r.t);
                                if posted.get() {
                                    crate::log_err!(post(&r), "re-posting the binding request to fgtw.org");
                                }
                                req = r;
                            }
                            last_repost = std::time::Instant::now();
                        }
                        // else: just loop and re-poll current_members (the fast path)
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        std::thread::sleep(crate::jitter_dur(std::time::Duration::from_secs(8)));
                        if last_repost.elapsed() >= REPOST_EVERY {
                            // Restamp the offer (registry + LAN, one stamp), then re-emit the beacon under the NEW eagle_time so the aired id keeps matching the entry the sponsor now reads.
                            if let Ok(r) = mint(&tx) {
                                crate::network::pairing_beacon::reannounce(&hp, &me, r.t);
                                if posted.get() {
                                    crate::log_err!(post(&r), "re-posting the binding request to fgtw.org");
                                }
                                req = r;
                            }
                            last_repost = std::time::Instant::now();
                        }
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::PhotonApp;

    /// The denied-requests list round-trips typed (ke leaver, hb salt, e6 stamp per request), and a blob that is not a `denied` field reads as empty, never as a guess.
    #[test]
    fn depart_denied_list_round_trips() {
        let list = vec![([1u8; 32], [2u8; 32], 9_999i64), ([3u8; 32], [4u8; 32], -5i64)];
        let blob = PhotonApp::encode_depart_denied(&list);
        assert_eq!(PhotonApp::decode_depart_denied(&blob), list);
        assert!(PhotonApp::decode_depart_denied(&[]).is_empty());
        let other = vsf::schema::section::FieldValue::new("recent", vec![vsf::VsfType::ke(vec![1u8; 32])]).flatten();
        assert!(PhotonApp::decode_depart_denied(&other).is_empty());
    }
}
