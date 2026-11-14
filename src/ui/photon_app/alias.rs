//! The app's side of Photon's alias (network/fgtw/alias.rs holds the work): start the settle off the UI thread, take its answer, keep the alias's device list in the vault, and let the list's devices be siblings.
//! The desktop and Android settle the person key and sign their own binding; ferros's Photon was handed both (PhotonKeys) and only joins the list.

use super::PhotonApp;

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
                std::thread::Builder::new().name("alias".into()).spawn(move || {
                    let list = crate::network::fgtw::alias::join_list(&keys, &device);
                    crate::log_err!(tx.send(Ok(crate::network::fgtw::alias::AliasReady { keys, list })), "the alias list to the UI thread");
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
                std::thread::Builder::new().name("alias".into()).spawn(move || {
                    let r = crate::network::fgtw::alias::settle(&session.identity_seed, &session.handle_proof, &fleet_key, &bundle, &secret);
                    crate::log_err!(tx.send(r), "the alias settle to the UI thread");
                })
            }
        };
        match spawned {
            Ok(_) => {
                crate::logf!("ALIAS: settling ({})", why);
                self.alias_rx = Some(rx);
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
        let ready = match r {
            Ok(ready) => ready,
            Err(e) => {
                crate::logf!("ALIAS: not settled: {}", e);
                return true;
            }
        };
        crate::logf!("ALIAS: settled; alias {}", hex::encode(&fgtw::alias::bundle(&ready.keys.alias).public().ed25519()[..4]));
        if self.photon_keys.is_none() {
            self.set_photon_keys(ready.keys);
        }
        if let Some((binding, list)) = ready.list {
            self.keep_alias_list(binding, list);
        }
        true
    }

    /// The vault entry the alias's device list (its slot document) is kept at.
    fn alias_list_addr(&self) -> Option<[u8; 32]> {
        self.messenger_root().map(|root| crate::storage::vault_key("alias_devices", &root))
    }

    /// Keep a checked list: held here, written to the vault at once, and the siblings reconciled so a newly listed device becomes one.
    fn keep_alias_list(&mut self, binding: Vec<u8>, list: fgtw::alias_devices::DeviceList) {
        if self.alias_list.as_ref().is_some_and(|(_, held)| !list.newer_than(held)) {
            return;
        }
        if let (Some(addr), Some(vault)) = (self.alias_list_addr(), crate::storage::device_vault()) {
            match fgtw::alias_devices::slot_doc(&binding, &list, crate::network::time_base::now_osc()) {
                Ok(doc) => {
                    crate::log_err!(vault.write_addr(&addr, &doc), "the alias device list to the vault");
                }
                Err(e) => crate::logf!("ALIAS: the list did not encode ({})", e),
            }
        }
        crate::logf!("ALIAS: device list revision {} holds {} device(s)", list.revision, list.devices.len());
        self.alias_list = Some((binding, list));
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
            Ok(slot) => self.alias_list = Some(slot),
            Err(e) => crate::logf!("ALIAS: the kept device list does not read ({})", e),
        }
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
        (keys.legacy_seed.is_none() || !fleet.contains(device)).then(|| keys.sibling_root())
    }
}
