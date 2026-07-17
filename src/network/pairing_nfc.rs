//! NFC instant device add — the proximity-secret carrier (Android only; docs/pairing-v2.md).
//!
//! The joiner generates a 32-byte random secret `S` per join session and serves it as a "dumb tag" over HCE under photon's PRIVATE AID (a generic tag reader never sees it — only an app that selects our AID). Its published binding request carries `pair::nfc_secret_hash(S, device_pubkey, t)` — bound to the candidate INSIDE the keyed hash, so a transplanted hash simply never matches (fail-closed).
//! The sponsor runs NFC reader mode while the AddDevice screen is up; a tap reads `S`, and the matcher hashes it against every candidate's published commitment — a match IS the proximity + intent proof, so the bind fires without typing the words (the green-confirm rotation gate still stands).
//! `S` MUST be 32 random bytes: the commitment is public, so a small preimage space would be offline-brute-forceable and void the whole gate.

use std::sync::Mutex;

/// A secret read by the sponsor's reader-mode tap, awaiting the tick drain. One deep — a second tap before the drain overwrites (same ceremony either way).
static SECRET_HEARD: Mutex<Option<[u8; 32]>> = Mutex::new(None);

/// Kotlin reader callback → here (via JNI). The tick drains with [`take_secret`].
pub fn on_secret_read(s: [u8; 32]) {
    *SECRET_HEARD.lock().unwrap() = Some(s);
}

/// Drain the last tapped secret (sponsor tick).
pub fn take_secret() -> Option<[u8; 32]> {
    SECRET_HEARD.lock().unwrap().take()
}

/// Serve `S` over HCE for as long as the guard lives — tie it to the join ceremony's scope so every exit path stops serving (mirror of the BLE AnnounceGuard).
pub fn serve_guard(secret: &[u8; 32]) -> ServeGuard {
    imp::start_serve(secret);
    ServeGuard(())
}

pub struct ServeGuard(());
impl Drop for ServeGuard {
    fn drop(&mut self) {
        imp::stop_serve();
    }
}

/// Sponsor: start/stop reader mode (the AddDevice tick lifecycle drives these, diffed like the beacon scan).
pub fn start_reader() {
    imp::start_reader();
}
pub fn stop_reader() {
    imp::stop_reader();
}

#[cfg(target_os = "android")]
mod imp {
    pub(super) fn start_serve(secret: &[u8; 32]) {
        crate::platform::jni_android::nfc_call_bytes("startServe", secret);
    }
    pub(super) fn stop_serve() {
        crate::platform::jni_android::nfc_call("stopServe");
    }
    pub(super) fn start_reader() {
        crate::platform::jni_android::nfc_call("startReader");
    }
    pub(super) fn stop_reader() {
        crate::platform::jni_android::nfc_call("stopReader");
    }
}

/// Every other platform: no NFC radio wired (desktop taps aren't a thing yet) — the words + BLE paths cover pairing there.
#[cfg(not(target_os = "android"))]
mod imp {
    pub(super) fn start_serve(_secret: &[u8; 32]) {}
    pub(super) fn stop_serve() {}
    pub(super) fn start_reader() {}
    pub(super) fn stop_reader() {}
}
