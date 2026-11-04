//! ANDROID ROOT CONVERSION PIN, MIGRATION-EXPIRES: 2027-02-01 (Nick 2026-11-02: thirty versions after v138).
//! A phone whose device vault was founded under the `ANDROID_ID` root moves onto the Widevine root (tohu/docs/oracle-hardening.md) once its session is live.
//! The old device key sponsors the new key into the fleet (the add is idempotent), the vault is re-homed entry by entry at the same addresses under the new secret, the new key countersigns the old key's departure, the old vault's files go, and the process exits so the next start selects the Widevine root and runs as the new device.
//! Nothing here reads a root itself: the selection at service start (jni_android.rs `select_android_root`) read both and left what is owed in [`note_owed`].
//! The selection always prefers the Widevine vault when it exists, so an interrupted conversion leaves a phone running as the new device with whatever was carried; [`Owed::Cleanup`] then finishes the fleet half and removes the old vault, and the fleet refills any entry the interruption skipped.
//! Logs carry BLAKE3 prefixes of device keys only, never a root.

use super::PhotonApp;
use crate::network::fgtw::fleet::{self, DeviceSigner, FleetSigner, SigningBundle};
use std::sync::Mutex;

/// What the root selection at service start found owed, for the session to settle.
pub enum Owed {
    /// Only a vault under the legacy root: this process runs as the legacy device; once the session is live, move everything to the Widevine root.
    Convert { legacy: Vec<u8>, widevine: Vec<u8> },
    /// A vault under both roots: this process runs on the Widevine root; the legacy device may still sit in the fleet and its vault still on disk.
    Cleanup { legacy: Vec<u8>, widevine: Vec<u8> },
}

static OWED: Mutex<Option<Owed>> = Mutex::new(None);

/// Parked by the worker once the new key is in the chain: the UI thread, which owns the sibling addresses, pushes the extended chain to every sibling and answers here, and only then does the worker carry on.
/// Without this the old device exited before any sibling heard the add, and the desktop took 82 minutes to find the new key at the seed, rejecting the phone's history requests until then (2026-11-02).
static CHAIN_PUSH: Mutex<Option<std::sync::mpsc::Sender<()>>> = Mutex::new(None);

/// The root selection hands over what it found; taken once by [`PhotonApp::root_conversion_tick`].
pub fn note_owed(owed: Owed) {
    if let Ok(mut g) = OWED.lock() {
        *g = Some(owed);
    }
}

impl PhotonApp {
    /// On the status tick, once per process: when the session is live and the device vault is open, start whatever the root selection left owed.
    /// The edge is the first tick with a session and an open vault; nothing is timed.
    pub(super) fn root_conversion_tick(&mut self) {
        // The worker's one request of this thread: push the chain that now holds the new key to the siblings, then let it carry on.
        let parked = CHAIN_PUSH.lock().ok().and_then(|mut g| g.take());
        if let Some(done) = parked {
            self.fleet_chain_push(&[]);
            // The contacts too: a key move on a one-device fleet leaves no device they know to pong the new tip at them (field 2026-11-04).
            self.fleet_chain_push_to_contacts();
            self.fleet_chain_store();
            crate::log_err!(done.send(()), "the chain-push answer to the root conversion worker");
        }
        if self.root_conversion_started {
            return;
        }
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let (Some(ours), Some(hp)) = (self.device_signer(), self.our_handle_proof()) else {
            return;
        };
        if crate::storage::device_vault_if_open().is_none() {
            return;
        }
        let owed = OWED.lock().ok().and_then(|mut g| g.take());
        self.root_conversion_started = true;
        let Some(owed) = owed else {
            return;
        };
        let identity_seed = session.identity_seed;
        let spawned = std::thread::Builder::new()
            .name("root-conversion".into())
            .spawn(move || match owed {
                Owed::Convert { legacy, widevine } => convert(&ours, &identity_seed, &hp, &legacy, &widevine),
                Owed::Cleanup { legacy, widevine } => cleanup(&ours, &identity_seed, &hp, &legacy, &widevine),
            });
        if let Err(e) = spawned {
            crate::logf!("ROOT: the conversion worker could not start ({e}); it is retried at the next start");
        }
    }
}

/// The signer a root derives: the PQ bundle, whose Ed25519 half is the device key.
fn signer_of(root: &[u8]) -> DeviceSigner {
    DeviceSigner::Pq(SigningBundle::derive(root))
}

fn pubkey_of(signer: &DeviceSigner) -> [u8; 32] {
    signer.keypair().public.to_bytes()
}

/// This process runs as the legacy device: carry the identity to the Widevine root and exit.
fn convert(old: &DeviceSigner, identity_seed: &[u8; 32], hp: &[u8; 32], legacy: &[u8], widevine: &[u8]) {
    let new = signer_of(widevine);
    let (old_pub, new_pub) = (pubkey_of(old), pubkey_of(&new));
    crate::logf!("ROOT: moving device {} onto the Widevine root as device {}", crate::fp(&old_pub), crate::fp(&new_pub));
    // 1. The old key sponsors the new key into the fleet; a crash after this and before the vault is carried lands back here, where the add is idempotent.
    let req = fleet::make_bind_request(&new, identity_seed, hp, &[0u8; 32]);
    if let Err(e) = fleet::bind_device_held(old, hp, &req) {
        crate::logf!("ROOT: the fleet add of the new device failed ({e}); nothing moved, retried at the next start");
        return;
    }
    // The siblings hear the add now, from this device while it is still a member and still running: the UI thread pushes the chain on its next tick and answers; the seed mirror alone took the desktop 82 minutes to notice.
    let (pushed_tx, pushed_rx) = std::sync::mpsc::channel();
    if let Ok(mut g) = CHAIN_PUSH.lock() {
        *g = Some(pushed_tx);
    }
    if pushed_rx.recv().is_err() {
        crate::log("ROOT: the chain push never answered; the move is retried at the next start");
        return;
    }
    crate::log("ROOT: the extended chain went to the siblings");
    // 2. The vault, entry by entry, at the same addresses.
    let new_secret = tohu::device::device_secret_from(widevine);
    let Some(old_vault) = crate::storage::device_vault() else {
        crate::log("ROOT: the device vault is not open; retried at the next start");
        return;
    };
    let carried = kete::FlatStorage::open_device_shared(crate::storage::APP, new_secret)
        .map_err(|e| e.to_string())
        .and_then(|v| v.set_identity(*identity_seed).map(|_| v).map_err(|e| e.to_string()))
        .and_then(|v| old_vault.rehome_into(&v).map_err(|e| e.to_string()));
    match carried {
        Ok(n) => crate::logf!("ROOT: vault carried: {} device-scope + {} identity-scope entries, {} left unopened", n.device, n.identity, n.unopened),
        Err(e) => {
            // A half-written vault under the new root would be selected first at the next start, so it goes now and the next start lands back here.
            crate::logf!("ROOT: the vault re-home failed ({e}); the new vault is discarded and the move is retried at the next start");
            crate::log_err!(kete::discard_device_vault(crate::storage::APP, &new_secret), "discarding the half-carried vault");
            return;
        }
    }
    // 3. The old device leaves, the new device countersigns: the bilateral removal with both halves on one phone.
    depart_old(old, &new, hp);
    // 4. The old vault's files go; this process still holds them open and exits in the next line.
    crate::log_err!(kete::discard_device_vault(crate::storage::APP, &tohu::device::device_secret_from(legacy)), "discarding the legacy vault");
    crate::log("ROOT: moved; exiting so the next start runs on the Widevine root (the session is typed once more)");
    // The log batches in RAM and process::exit runs no destructor, so the lines above reach the disk here or never.
    crate::flush_log_buffer();
    std::process::exit(0);
}

/// This process runs on the Widevine root beside a leftover legacy vault: finish the fleet half and remove the vault.
fn cleanup(ours: &DeviceSigner, identity_seed: &[u8; 32], hp: &[u8; 32], legacy: &[u8], widevine: &[u8]) {
    let old = signer_of(legacy);
    let (old_pub, our_pub) = (pubkey_of(&old), pubkey_of(ours));
    if our_pub != pubkey_of(&signer_of(widevine)) {
        crate::log("ROOT: this process does not run as the Widevine device; cleanup left for the next start");
        return;
    }
    crate::logf!("ROOT: finishing the move from device {} to device {}", crate::fp(&old_pub), crate::fp(&our_pub));
    // The add is idempotent, so a conversion interrupted before the new device was in the fleet is completed here by the legacy key, still readable until the pin goes.
    let req = fleet::make_bind_request(ours, identity_seed, hp, &[0u8; 32]);
    if let Err(e) = fleet::bind_device_held(&old, hp, &req) {
        crate::logf!("ROOT: the fleet add of this device failed ({e}); retried at the next start");
        return;
    }
    depart_old(&old, ours, hp);
    crate::log_err!(kete::discard_device_vault(crate::storage::APP, &tohu::device::device_secret_from(legacy)), "discarding the legacy vault");
}

/// The old device's consent to leave, countersigned and published by the new device.
fn depart_old(old: &DeviceSigner, new: &DeviceSigner, hp: &[u8; 32]) {
    let t = vsf::eagle_time_oscillations();
    let consent = fgtw::protocol::depart_consent(old, hp, t);
    match fleet::depart_device_consented(new, hp, &pubkey_of(old), t, &consent) {
        Ok(()) => crate::logf!("ROOT: device {} left the fleet", crate::fp(&pubkey_of(old))),
        Err(e) => crate::logf!("ROOT: the old device's departure did not publish ({e}); retried at the next start"),
    }
}
