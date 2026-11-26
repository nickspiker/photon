//! The app's side of Photon's alias (network/fgtw/alias.rs holds the work): start the settle off the UI thread, take its answer, keep the alias's device list in the vault, and let the list's devices be siblings.
//! The desktop and Android settle the person key and sign their own binding; ferros's Photon was handed both (PhotonKeys) and only joins the list.

use super::{tr, Msg, PhotonApp};

impl PhotonApp {
    /// Settle the alias off the UI thread: on the desktop and Android once the session and the fleet key are in hand (after an attest or a resume), on ferros once PhotonKeys came in. A settle already running is left to finish.
    pub(super) fn spawn_alias_settle(&mut self, why: &str) {
        if self.alias_rx.is_some() {
            return;
        }
        let Some(kp) = self.device_keypair.clone() else {
            return;
        };
        let device = kp.public.to_bytes();
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = match self.photon_keys.as_ref().filter(|k| k.legacy_seed.is_none()) {
            // ferros: the alias and binding were handed in; only the list is ours to join.
            Some(keys) => {
                let keys = keys.clone();
                let kept = self.alias_kept();
                std::thread::Builder::new().name("alias".into()).spawn(move || {
                    let joined = crate::network::fgtw::alias::join_list(&keys, &device, &kept);
                    crate::log_err!(tx.send(Ok(crate::network::fgtw::alias::AliasReady { keys, joined, own: Vec::new() })), "the alias list to the UI thread");
                })
            }
            None => {
                let (Some(session), Some(bundle)) = (self.session.clone(), self.signing_bundle.clone()) else {
                    return;
                };
                let Some(fleet_key) = crate::storage::device_vault()
                    .and_then(|v| v.read_addr(&crate::storage::vault_key("fleet_key", &session.vault_seed)).ok().flatten())
                    .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
                else {
                    crate::log("ALIAS: no fleet key held yet; the alias settles once it is");
                    return;
                };
                let secret = *kp.secret.as_bytes();
                let kept = self.alias_kept();
                let own = self.own_profile_stamped();
                std::thread::Builder::new().name("alias".into()).spawn(move || {
                    let r = crate::network::fgtw::alias::settle(&session.identity_seed, &session.vault_seed, &session.handle_proof, &fleet_key, &bundle, &secret, &kept, &own);
                    crate::log_err!(tx.send(r), "the alias settle to the UI thread");
                })
            }
        };
        match spawned {
            Ok(_) => {
                crate::logf!("ALIAS: settling ({})", why);
                self.alias_rx = Some(rx);
                // This settle carries our profile as it stands now.
                self.self_owed = false;
            }
            Err(e) => crate::logf!("ALIAS: the settle thread did not start ({})", e),
        }
    }

    /// Take the settle's answer: the keys (the desktop and Android hold the alias from now on; nothing moves while the identity seed is held), the device list kept in the vault, and the siblings reconciled against it. True when it landed.
    pub(super) fn drain_alias(&mut self) -> bool {
        let Some(rx) = self.alias_rx.as_ref() else {
            return false;
        };
        let r = match rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("the settle thread ended without an answer".to_string()),
        };
        self.alias_rx = None;
        let mut ready = match r {
            Ok(ready) => ready,
            Err(e) => {
                crate::logf!("ALIAS: not settled: {}", e);
                self.alias_owe();
                return true;
            }
        };
        crate::logf!("ALIAS: settled; alias {}", hex::encode(&fgtw::alias::ed25519_public(&ready.keys.alias)[..4]));
        // SELF as the space holds it now: taken where it is newer than our own settings, and kept as what the space last said, so a later edit here knows it is owed.
        let own = std::mem::take(&mut ready.own);
        if !self.alias_alone() {
            self.self_settled = own.clone();
            self.adopt_self(own);
        }
        if self.photon_keys.is_none() {
            self.set_photon_keys(ready.keys);
            // The alias is in hand from here: the pong seals for alias siblings switch to the alias's sibling secret, and each alias sibling gets our fleet state again under the alias's state key. A push before this went under the fleet key, which an alias sibling cannot open (the desktop's push to the Pro at its online edge, 2026-11-17).
            self.reseed_contact_pubkeys();
            let fleet = self.fleet_members_held();
            let alias_sibs: Vec<[u8; 32]> = self.contacts.iter().filter(|c| c.is_sibling && !c.locked_out).filter_map(|c| c.device_key()).filter(|d| self.alias_sibling_root(d, &fleet).is_some()).collect();
            for d in alias_sibs {
                self.fleet_state_push(Some(d));
            }
        }
        // Every settle: the alias siblings' pair secrets, which their frames seal under (alias_seal_key).
        self.refresh_alias_pairs();
        let joined = ready.joined;
        self.adopt_listed_locked(joined.listed_locked);
        if joined.self_locked {
            if self.alias_alone() {
                crate::log("ALIAS: our chain has locked this Photon out; discarding its state here");
                self.spawn_alias_sign_out(false);
            }
            return true;
        }
        match joined.list {
            Some((binding, list)) => {
                self.alias_retry = None;
                self.keep_alias_list(binding, list, joined.signouts);
            }
            None => self.alias_owe(),
        }
        self.redrive_alias_locks();
        // Our profile moved while that settle ran: one more carries it.
        if self.self_owed {
            self.spawn_alias_settle("our profile changed during the settle");
        }
        true
    }

    /// The chain's locks on devices outside the membership, as a fold just read them: kept in RAM, and on a change the locked set is applied again, so a newly locked device is refused, its row flagged and every friendship's re-key armed on every device of ours.
    pub(super) fn adopt_listed_locked(&mut self, listed: Vec<[u8; 32]>) {
        let mut merged = self.chain_listed_locked.clone();
        for d in listed {
            if !merged.contains(&d) {
                merged.push(d);
            }
        }
        if merged != self.chain_listed_locked {
            crate::logf!("ALIAS: our chain locks {} device(s) outside the membership", merged.len());
            self.chain_listed_locked = merged;
            self.apply_locked_set();
        }
    }

    /// LOCK OUT an alias sibling (the owner 2026-11-18: "triggers a re-key and locks out that device"), from a member of the fleet: refused here at once (its row flagged, every friendship's re-key armed, nothing sealed to it again), then off the UI thread the chain records the lock (OpKind::LockListed, the record a stolen alias holder cannot write) and the list is published without it.
    pub(super) fn lock_out_alias_device(&mut self, pk: [u8; 32]) {
        if self.alias_lock_rx.is_some() {
            crate::log("ALIAS: a lock out is already running; this one waits for it");
            return;
        }
        let (Some(hp), Some(signer), Some(kp)) = (self.our_handle_proof(), self.device_signer(), self.device_keypair.as_ref()) else {
            crate::log("ALIAS: lock out needs our identity and this device's signer; not held yet");
            return;
        };
        // The alias prunes the list; without it yet the chain's lock alone is written, and the settle that brings the alias prunes the list (join_list leaves locked devices out).
        let keys = self.photon_keys.clone();
        let kept = self.alias_kept();
        if self.alias_alone() {
            crate::log("ALIAS: Photon on the alias alone is no member of the chain and cannot lock a device out");
            return;
        }
        let me = kp.public.to_bytes();
        crate::logf!("ALIAS: locking out {} — refused here now, the chain and the list follow", crate::fp(&pk));
        if !self.chain_listed_locked.contains(&pk) {
            self.chain_listed_locked.push(pk);
        }
        self.alias_pairs.retain(|(d, _)| *d != pk);
        self.alias_chains_owed.retain(|d| *d != pk);
        self.apply_locked_set();
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = self.event_proxy.clone();
        let spawned = std::thread::Builder::new().name("alias lock".into()).spawn(move || {
            let r = crate::network::fgtw::fleet::lock_listed_device_chain(&signer, &hp, &pk).map(|listed_locked| match keys {
                Some(keys) => crate::network::fgtw::alias::join_list(&keys, &me, &kept),
                None => crate::network::fgtw::alias::Joined { listed_locked, ..Default::default() },
            });
            crate::log_err!(tx.send((pk, r)), "the lock out to the UI thread");
            if let Some(w) = wake {
                super::wake_ui(&w);
            }
        });
        match spawned {
            Ok(_) => self.alias_lock_rx = Some(rx),
            Err(e) => crate::logf!("ALIAS: the lock out thread did not start ({}); the next settle records it", e),
        }
    }

    /// A locked alias sibling row the chain does not hold the lock for yet (a lock out whose chain write did not land): recorded again, one at a time, on each settle.
    fn redrive_alias_locks(&mut self) {
        if self.alias_alone() || self.alias_lock_rx.is_some() {
            return;
        }
        let members = self.fleet_members_held();
        if members.is_empty() {
            return;
        }
        let unrecorded = self.contacts.iter().filter(|c| c.is_sibling && c.locked_out).filter_map(|c| c.device_key()).find(|d| !members.contains(d) && !self.chain_listed_locked.contains(d));
        if let Some(d) = unrecorded {
            crate::logf!("ALIAS: {} is locked here but not on our chain; recording it again", crate::fp(&d));
            self.lock_out_alias_device(d);
        }
    }

    /// SIGN OUT on Photon on the alias alone (the owner 2026-11-18: "the other needs the device present to sign itself out, thus asserting it wiped its keys"): the first tap arms, the second sends. The list is published without this device, then Photon's state here is discarded; nothing is re-keyed.
    pub(super) fn alias_sign_out_tap(&mut self) {
        if !self.alias_sign_out_armed {
            self.alias_sign_out_armed = true;
            return;
        }
        self.alias_sign_out_armed = false;
        self.spawn_alias_sign_out(true);
    }

    /// Off the UI thread: leave the list (when `publish`, the sign out; a locked out device has nothing to publish), discard every entry of Photon's vault here, and mark this device signed out of this person, so the keys handed in at the next start do not join it again.
    pub(super) fn spawn_alias_sign_out(&mut self, publish: bool) {
        if self.alias_sign_out_rx.is_some() {
            return;
        }
        let (Some(keys), Some(kp)) = (self.photon_keys.clone(), self.device_keypair.as_ref()) else {
            return;
        };
        // The sign out is signed by this device's own key, the one the list names it by.
        let device_key = crate::network::fgtw::fleet::signer_for(kp);
        let kept = self.alias_kept();
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = self.event_proxy.clone();
        let spawned = std::thread::Builder::new().name("alias sign out".into()).spawn(move || {
            let r = if publish { crate::network::fgtw::alias::leave_list(&keys, &device_key, &kept) } else { Ok(()) };
            let r = r.and_then(|()| discard_photon_vault(&keys.handle_proof));
            crate::log_err!(tx.send(r), "the sign out to the UI thread");
            if let Some(w) = wake {
                super::wake_ui(&w);
            }
        });
        match spawned {
            Ok(_) => self.alias_sign_out_rx = Some(rx),
            Err(e) => crate::logf!("ALIAS: the sign out thread did not start ({})", e),
        }
    }

    /// Take a lock out's or a sign out's answer. True when one landed.
    pub(super) fn drain_alias_removal(&mut self) -> bool {
        let mut landed = false;
        if let Some((pk, r)) = self.alias_lock_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.alias_lock_rx = None;
            landed = true;
            match r {
                Ok(joined) => {
                    crate::logf!("ALIAS: {} is locked out on our chain", crate::fp(&pk));
                    // The held chain carries the lock: kept in the vault and sent to the siblings and every contact.
                    self.fleet_chain_store();
                    if let Some(hp) = self.our_handle_proof() {
                        self.chain_news(hp);
                    }
                    self.adopt_listed_locked(joined.listed_locked);
                    if let Some((binding, list)) = joined.list {
                        self.keep_alias_list(binding, list, joined.signouts);
                    }
                    self.ready_toast = Some(tr(Msg::LockedOutToast).into_owned());
                }
                Err(e) => {
                    crate::logf!("ALIAS: the chain did not record the lock of {} ({}); it stays refused here and is recorded again at the next settle", crate::fp(&pk), e);
                    self.ready_toast = Some(tr(Msg::LockOutFailed).into_owned());
                }
            }
        }
        if let Some(r) = self.alias_sign_out_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.alias_sign_out_rx = None;
            landed = true;
            match r {
                Ok(()) => {
                    crate::log("ALIAS: signed out; Photon's state on this device is discarded");
                    self.photon_keys = None;
                    self.alias_list = None;
                    self.alias_signouts.clear();
                    self.alias_pairs.clear();
                    self.alias_chains_owed.clear();
                    self.chain_listed_locked.clear();
                    self.lockout_rekey_busy = None;
                    self.clean_device_for_reuse();
                }
                Err(e) => {
                    crate::logf!("ALIAS: the sign out did not complete ({}); nothing was discarded", e);
                    self.ready_toast = Some(tr(Msg::SignOutPublishFailed).into_owned());
                }
            }
        }
        landed
    }

    /// The vault entry the alias's device list (its slot document) is kept at.
    fn alias_list_addr(&self) -> Option<[u8; 32]> {
        self.messenger_root().map(|root| crate::storage::vault_key("alias_devices", &root))
    }

    /// The list kept here and the sign-outs beside it, for a join to measure fgtw.org's list against (network/fgtw/alias.rs Kept).
    fn alias_kept(&self) -> crate::network::fgtw::alias::Kept {
        crate::network::fgtw::alias::Kept { list: self.alias_list.as_ref().map(|(_, l)| l.clone()), signouts: self.alias_signouts.clone() }
    }

    /// Keep a checked list: held here, written to the vault at once with the sign-outs that back what it leaves out, and the siblings reconciled so a newly listed device becomes one and a signed-out or locked one goes.
    /// The join that hands it over never hands a list leaving out a device the kept list named without a backing (join_list), so a row goes here only on a device's own sign out or the chain.
    fn keep_alias_list(&mut self, binding: Vec<u8>, list: fgtw::alias_devices::DeviceList, signouts: Vec<fgtw::alias_devices::SignOut>) {
        if let Some((_, held)) = self.alias_list.as_ref().filter(|(_, held)| !list.newer_than(held)) {
            crate::logf!("ALIAS: device list revision {} ({} device(s)) is the one held; nothing to change", held.revision, held.devices.len());
            return;
        }
        if let (Some(addr), Some(vault)) = (self.alias_list_addr(), crate::storage::device_vault()) {
            match fgtw::alias_devices::slot_doc_with(&binding, &list, &signouts, crate::network::time_base::now_osc()) {
                Ok(doc) => {
                    crate::log_err!(vault.write_addr(&addr, &doc), "the alias device list to the vault");
                }
                Err(e) => crate::logf!("ALIAS: the list did not encode ({})", e),
            }
        }
        crate::logf!("ALIAS: device list revision {} holds {} device(s)", list.revision, list.devices.len());
        self.alias_list = Some((binding, list));
        self.alias_signouts = signouts;
        // The fleet's members as the held chain folds them, never the sibling rows: a locked-out device keeps its row but answers for nothing, and reconciling against the rows dropped it as "left the fold" (1be949c1, 2026-11-14).
        let members = self.fleet_members_held();
        // A member Photon whose chain is not held yet reads no members, and a reconcile then would drop every member sibling: it waits for the fold's own reconcile. This device is always a member, so empty means not held.
        if members.is_empty() && !self.photon_keys.as_ref().is_some_and(|k| k.legacy_seed.is_none()) {
            return;
        }
        self.reconcile_fleet_siblings(&members);
    }

    /// The list as the vault last kept it, read at resume so the siblings it names are known before the network answers.
    pub(super) fn load_alias_list(&mut self) {
        let Some(addr) = self.alias_list_addr() else {
            return;
        };
        let Some(doc) = crate::storage::device_vault().and_then(|v| v.read_addr(&addr).ok().flatten()) else {
            return;
        };
        match fgtw::alias_devices::from_slot_doc(&doc) {
            Ok(slot) => {
                self.alias_list = Some(slot);
                self.alias_signouts = fgtw::alias_devices::signouts_from_slot_doc(&doc).unwrap_or_default();
            }
            Err(e) => crate::logf!("ALIAS: the kept device list does not read ({})", e),
        }
    }

    /// Signed in: a session (the desktop and Android) or PhotonKeys handed in (ferros, which never has a session). Every screen routing that asked `session.is_some()` asks this, or ferros Photon fell to the handle screen on a Back from Settings (2026-11-14).
    pub(super) fn signed_in(&self) -> bool {
        self.session.is_some() || self.photon_keys.is_some()
    }

    /// Where a Photon not signed in lands: the handle screen, except on ferros, which has none and waits for its keys.
    pub(super) fn not_signed_in_screen(&self) -> super::AppState {
        #[cfg(target_os = "ferros")]
        return super::AppState::Launch(super::LaunchState::WaitingForIdentity);
        #[cfg(not(target_os = "ferros"))]
        super::AppState::Launch(super::LaunchState::Fresh)
    }

    /// A sibling that waits for the alias's list: one that never wove, under a held list that does not name it. A fleet member that is not a Photon (ferros's identity service, baf3dec7) never joins the list and never answers a ceremony; offering it one cost a 573 KB offer it took whole and died on (the Pro, 2026-11-17). A new Photon of ours joins the list at its first settle and weaves then; a sibling that already wove is never held back.
    pub(super) fn sibling_awaits_list(&self, c: &crate::types::Contact) -> bool {
        c.is_sibling
            && c.clutch_state != crate::types::ClutchState::Complete
            && self.alias_list.is_some()
            && c.device_key().is_some_and(|d| !self.alias_listed_devices().contains(&d))
    }

    /// A sibling of ours that is a Photon on the alias alone (a ferros one): the list names it, or our chain locked it as such, and the chain does not hold it as a member. Its Lock out is the chain's LockListed, never Revoke. False until our chain is held (alias_only_sibling).
    pub(super) fn is_alias_only_device(&self, device: &[u8; 32]) -> bool {
        // The fold as last adopted, never a fresh fold: the Fleet page asks this per row per frame.
        (self.alias_listed_devices().contains(device) || self.chain_listed_locked.contains(device)) && alias_only_sibling(self.alias_alone(), &self.registry_converged_fold, device)
    }

    /// The devices the alias's list names (this device among them).
    pub(super) fn alias_listed_devices(&self) -> Vec<[u8; 32]> {
        self.alias_list.as_ref().map(|(_, l)| l.devices.clone()).unwrap_or_default()
    }

    /// The devices our fleet's chain holds as members, as this device holds the chain (empty on the alias alone, where Photon holds none).
    pub(super) fn fleet_members_held(&self) -> Vec<[u8; 32]> {
        if self.photon_keys.as_ref().is_some_and(|k| k.legacy_seed.is_none()) {
            return Vec::new();
        }
        self.our_handle_proof().and_then(|hp| crate::network::fgtw::fleet::held_chain(&hp)).and_then(|c| c.fold().ok()).unwrap_or_default()
    }

    /// The secret shared with sibling `device` when it is an alias sibling: a Photon of ours that is not a member of the fleet (a ferros one), or any sibling when this Photon is on the alias alone. Both ends reach the same answer: the ferros end always mixes the alias, and the member end mixes it for a sibling the chain does not hold. None for a sibling that shares the identity seed (both members of the fleet).
    pub(super) fn alias_sibling_root(&self, device: &[u8; 32], fleet: &[[u8; 32]]) -> Option<[u8; 32]> {
        let keys = self.photon_keys.as_ref()?;
        alias_only_sibling(keys.legacy_seed.is_none(), fleet, device).then(|| keys.sibling_root())
    }

    /// The seal for a frame of `purpose` to alias sibling `device` (2026-11-17, the owner: the person's conversations on a ferros Photon, and a way to cut a lost one off): a key from the pair secret our sibling ceremony made with that one device (fanout_pairs, kept in `alias_pairs`), so no fleet key ever reaches an alias sibling, and removing it from the alias's list stops every frame to it. None for a fleet sibling, or an alias sibling whose ceremony has not completed.
    /// The friendship chains themselves do reach it, sealed under this key (the owner 2026-11-18: "friendship keys get copied to every device"); what a removed device keeps of them is said in drive_chain_replication.
    pub(super) fn alias_seal_key(&self, device: &[u8; 32], purpose: &[u8]) -> Option<[u8; 32]> {
        let (_, pair) = self.alias_pairs.iter().find(|(d, _)| d == device)?;
        Some(alias_frame_key(pair, purpose))
    }

    /// Photon on the alias alone (a ferros one): PhotonKeys handed in, no identity seed, never a member of the fleet.
    /// No friend's device takes a frame from it until contacts accept photons from the alias's device list, so it never starts a friend ceremony or sends a friend anything itself: its sends go thru an alias sibling (f43fba37's forward), even while it holds every friendship's chains.
    pub(super) fn alias_alone(&self) -> bool {
        self.photon_keys.as_ref().is_some_and(|k| k.legacy_seed.is_none())
    }

    /// Owe alias sibling `device` every friendship's chains, served whole by the next replication pass under its pair key: when its pair secret is first read here, and each time it comes online.
    /// Only a fleet member serves this way; Photon on the alias alone pushes what it advances, and its siblings hold everything already.
    pub(super) fn owe_alias_chains(&mut self, device: [u8; 32]) {
        if self.alias_alone() || !self.alias_pairs.iter().any(|(d, _)| *d == device) || self.alias_chains_owed.contains(&device) {
            return;
        }
        self.alias_chains_owed.push(device);
    }

    /// Read the pair secrets of every alias sibling into `alias_pairs`, off the UI thread (a vault read each): on the alias's settle, on resume, and when a sibling ceremony completes.
    pub(super) fn refresh_alias_pairs(&mut self) {
        let fleet = self.fleet_members_held();
        let devices: Vec<[u8; 32]> = self
            .contacts
            .iter()
            .filter(|c| c.is_sibling && !c.locked_out)
            .filter_map(|c| c.device_key())
            .filter(|d| self.alias_sibling_root(d, &fleet).is_some())
            .collect();
        let (Some(ours), Some(storage)) = (self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes()), self.storage.clone()) else {
            return;
        };
        if devices.is_empty() {
            self.alias_pairs.clear();
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = self.event_proxy.clone();
        crate::log_err!(std::thread::Builder::new().name("alias pairs".into()).spawn(move || {
            let pairs: Vec<([u8; 32], [u8; 32])> = devices.iter().filter_map(|d| crate::storage::fanout_pairs::load(&ours, d, storage.as_ref()).map(|p| (*d, p))).collect();
            crate::log_err!(tx.send(pairs), "the alias siblings' pair secrets to the UI");
            if let Some(w) = wake {
                super::wake_ui(&w);
            }
        }), "spawn the alias pair read");
        self.alias_pairs_rx = Some(rx);
    }

    /// Take the pair secrets the worker read. True when they landed.
    pub(super) fn drain_alias_pairs(&mut self) -> bool {
        let Some(pairs) = self.alias_pairs_rx.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return false;
        };
        self.alias_pairs_rx = None;
        crate::logf!("ALIAS: {} alias sibling(s) with a pair secret: frames to them seal under it", pairs.len());
        let fresh: Vec<[u8; 32]> = pairs.iter().map(|(d, _)| *d).filter(|d| !self.alias_pairs.iter().any(|(x, _)| x == d)).collect();
        self.alias_pairs = pairs;
        // A device whose pair secret is new here could open none of our chains before: it is owed all of them, and our address book, which it cannot ask fgtw.org for.
        for d in fresh.iter().copied() {
            self.owe_alias_chains(d);
            self.addr_book_push(Some(d), "its pair secret was read here");
        }
        if !fresh.is_empty() {
            // A pair key read is a key edge: a fetch no device could open may find one now.
            self.attach_address_edge("an alias sibling's pair secret was read");
            // Our own picture from the siblings now able to answer, when this device holds none or, on the alias alone, the fleet's stamp moved (conversation.rs want_own_avatar_from_siblings).
            self.want_own_avatar_from_siblings();
        }
        true
    }

    /// A settle or join that did not reach fgtw.org (ferros's Pro lost its WiFi at the one moment it tried, 2026-11-14): owed again on the next network edge, each try at least twice as far from the last as the one before, so a down fgtw.org is never hammered. Nothing waits on a clock: an edge that comes too soon is let pass, the next one tries.
    fn alias_owe(&mut self) {
        let failures = self.alias_retry.map_or(1, |(f, _)| f + 1);
        // WHY the min: the shift's amount must stay under the counter's width; past 2^6 seconds the gap stops growing (an edge is still what tries).
        let gap = (crate::OSC_PER_SEC as i64) << (failures - 1).min(6);
        self.alias_retry = Some((failures, crate::network::time_base::now_osc() + gap));
    }

    /// A network edge (a peer answered, the interface changed, ferros's route flags moved): an owed settle runs now if its gap has passed.
    pub(super) fn alias_retry_edge(&mut self, why: &str) {
        if self.alias_retry.is_some_and(|(_, not_before)| crate::network::time_base::now_osc() >= not_before) {
            self.spawn_alias_settle(why);
        }
    }

    /// ferros's network edge: the ip server's flags changed since the last tick looked.
    #[cfg(target_os = "ferros")]
    pub(super) fn alias_net_tick(&mut self) {
        let generation = crate::platform::ferros_net::network_generation();
        if generation != self.alias_net_generation {
            self.alias_net_generation = generation;
            self.alias_retry_edge("the network changed");
        }
    }

    /// ferros (PHOTON.md step 4): the person came in as PhotonKeys with no identity seed, so there is no handle screen and no FGTW part here: the vault opens under the messenger root, off the UI thread, and the resume lands Photon on its contacts. At init when the keys came before it, or when they come later to a Photon waiting for them (set_photon_keys).
    pub(super) fn open_with_photon_keys(&mut self) {
        let Some(root) = self.photon_keys.as_ref().filter(|k| k.legacy_seed.is_none()).map(|k| k.messenger_root()) else {
            return;
        };
        let Some(kp) = &self.device_keypair else {
            return;
        };
        let device_secret = *kp.secret.as_bytes();
        let hp = self.photon_keys.as_ref().map(|k| k.handle_proof).unwrap_or_default();
        let t_boot = std::time::Instant::now();
        let (tx, rx) = std::sync::mpsc::channel();
        // The worker reads the contacts and their conversations' summaries too, so the first Ready frame waits on no vault call here (rows_load.rs).
        let pids = self.resume_pids();
        let spawned = std::thread::Builder::new()
            .name("vault-open".into())
            .spawn(move || {
                // A device signed out of this person opens nothing for it: the vault was discarded, and opening it would join the list again.
                crate::storage::install_device_secret(device_secret);
                if signed_out_of(&hp) {
                    SIGNED_OUT_HELD.store(true, std::sync::atomic::Ordering::Release);
                    crate::log_err!(tx.send(Err(crate::storage::StorageError::Vault("this device signed out of this person".to_string()))), "the sign out mark to the UI thread");
                    return;
                }
                crate::log_err!(tx.send(super::rows_load::open_and_load_contacts(crate::storage::open_session_vault(root, root, device_secret), &pids, t_boot)), "the opened vault to the UI thread");
            })
            .is_ok();
        if spawned {
            self.resume_vault_rx = Some((rx, None, t_boot));
            crate::log("RESUME: PhotonKeys in hand; the vault opens under the messenger root");
            self.show_opening();
        } else {
            let opened = super::rows_load::open_and_load_contacts(crate::storage::open_session_vault(root, root, device_secret), &pids, t_boot);
            self.finish_resume_load(None, opened, t_boot);
        }
    }
}

/// The key a frame of `purpose` to or from an alias sibling seals under: hashed from the pair secret our sibling ceremony made with that one device, so each purpose and each device has its own.
pub(super) fn alias_frame_key(pair: &[u8; 32], purpose: &[u8]) -> [u8; 32] {
    let mut h = blake3::Hasher::new_derive_key("photon 2026-11-17 frames to an alias sibling");
    h.update(purpose);
    h.update(pair);
    *h.finalize().as_bytes()
}

/// Is sibling `device` one we share only the alias with: every sibling on the alias alone; on a member Photon, a sibling the chain's members do not include.
/// A member Photon is always in its own chain, so no members means the held chain has not loaded yet: until that edge no sibling is read as alias-only (5b1926dc's startup race: read so in that window, a fleet sibling's frames sealed under a pair key it never made).
pub(super) fn alias_only_sibling(alias_alone: bool, fleet: &[[u8; 32]], device: &[u8; 32]) -> bool {
    alias_alone || (!fleet.is_empty() && !fleet.contains(device))
}

/// Discard every entry of Photon's vault on this device (the owner 2026-09-27: deletion is a mark for discard, never a shred claim) and mark the device signed out of the person `handle_proof`, so the keys handed in at the next start do not join it again. Off the UI thread.
fn discard_photon_vault(handle_proof: &[u8; 32]) -> Result<(), String> {
    let vault = crate::storage::device_vault().ok_or("no vault")?;
    let addrs = vault.live_addrs().map_err(|e| format!("the vault's entries did not list ({e})"))?;
    vault.delete_addr_batch(&addrs).map_err(|e| format!("the vault's entries were not discarded ({e})"))?;
    vault.write_device(SIGNED_OUT_FLAG, handle_proof).map_err(|e| format!("the sign out mark was not written ({e})"))?;
    crate::logf!("ALIAS: {} vault entries discarded; this device is signed out of its person", addrs.len());
    Ok(())
}

/// The device-scope entry naming the person this device signed out of (their handle proof, 32 bytes): set by a sign out, read where the keys are handed in.
const SIGNED_OUT_FLAG: &str = "flags/signed_out";

/// The vault-open worker found this device signed out of the person whose keys came in: the resume drops the keys instead of reporting a lost vault.
pub(super) static SIGNED_OUT_HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether this device signed out of the person `handle_proof`: a Photon handed that person's keys again stays out of it. Off the UI thread (a vault read).
pub(super) fn signed_out_of(handle_proof: &[u8; 32]) -> bool {
    crate::storage::device_vault().is_some_and(|v| matches!(v.read_device(SIGNED_OUT_FLAG), Ok(Some(b)) if b.as_slice() == handle_proof))
}

/// May a frame on a conversation's keys leave here for these recipients: anything from a fleet member; from Photon on the alias alone, only what goes to its own siblings (no friend's device takes a frame from it yet, whatever chains it holds).
pub(super) fn alias_may_send(alias_alone: bool, every_recipient_ours: bool) -> bool {
    !alias_alone || every_recipient_ours
}

/// May a ceremony with this contact start here (a keygen, an offer, a re-key): a sibling's weave always; a friend's never on the alias alone, chains held or not (PHOTON.md group 0 brings the friend CLUTCH on the alias).
pub(super) fn ceremony_may_start(alias_alone: bool, is_sibling: bool) -> bool {
    is_sibling || !alias_alone
}

#[cfg(test)]
mod tests {
    use super::{alias_frame_key, alias_may_send, alias_only_sibling, ceremony_may_start};
    use super::super::sync::{open_chain_sync, seal_chain_sync};

    /// The startup race (5b1926dc): on a member Photon, before its chain loads, no sibling is read as alias-only, so no fleet sibling's frame seals under a pair key it never made; once the chain is held, a sibling outside it is an alias one; on the alias alone every sibling is.
    #[test]
    fn no_alias_classification_before_the_chain_loads() {
        let (me, fleet_sib, ferros) = ([1u8; 32], [2u8; 32], [3u8; 32]);
        assert!(!alias_only_sibling(false, &[], &fleet_sib), "chain not loaded: a fleet sibling is not misread");
        assert!(!alias_only_sibling(false, &[], &ferros), "chain not loaded: nothing is classified yet");
        assert!(!alias_only_sibling(false, &[me, fleet_sib], &fleet_sib));
        assert!(alias_only_sibling(false, &[me, fleet_sib], &ferros));
        assert!(alias_only_sibling(true, &[], &fleet_sib));
    }

    fn keypair(seed: u8) -> ([u8; 32], [u8; 32]) {
        let sk = ed25519_dalek::SigningKey::from_bytes(&[seed; 32]);
        (sk.verifying_key().to_bytes(), [seed; 32])
    }

    /// A friendship's chains to an alias sibling: sealed under the pair key, carried in a signed chain_sync frame naming epoch 0, opened under the same pair key and nothing else.
    #[test]
    fn chains_round_trip_under_the_pair_key() {
        let (pubkey, secret) = keypair(23);
        let pair = [0x5Au8; 32];
        let token = [0xC3u8; 32];
        let chains = vec![0x71u8; 4096];
        let key = alias_frame_key(&pair, b"chain_sync");
        let frame = seal_chain_sync(&chains, &token, &key, 0, &pubkey, &secret).unwrap();
        let ((ptok, pek, sealed), signer) = crate::network::fgtw::protocol::parse_chain_sync_vsf(&frame).unwrap();
        assert_eq!((ptok, pek, signer), (token, 0, pubkey));
        assert_eq!(open_chain_sync(&sealed, &[key]), Some(chains.clone()));
        // Another device's pair, another purpose's key, or the fleet's epoch key opens nothing.
        let other_pair = alias_frame_key(&[0x5Bu8; 32], b"chain_sync");
        let other_purpose = alias_frame_key(&pair, b"hist_page");
        let epoch = crate::crypto::clutch::fleet_epoch_seal_key(&[0x11u8; 32], b"chain_sync");
        assert_eq!(open_chain_sync(&sealed, &[other_pair, other_purpose, epoch]), None);
        // The receiver tries each key it holds for the sender; the pair key opens wherever it sits among them.
        assert_eq!(open_chain_sync(&sealed, &[epoch, key]), Some(chains));
    }

    /// Photon on the alias alone holding every friendship's chains: no direct send to a friend and no friend ceremony; its siblings are still reached, and their weaves still run. A fleet member is unchanged.
    #[test]
    fn the_alias_alone_reaches_only_its_siblings() {
        assert!(!alias_may_send(true, false));
        assert!(alias_may_send(true, true));
        assert!(alias_may_send(false, false));
        assert!(!ceremony_may_start(true, false));
        assert!(ceremony_may_start(true, true));
        assert!(ceremony_may_start(false, false));
    }
}
