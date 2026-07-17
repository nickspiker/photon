//! The device-binding marker — the client half of ONE-IDENTITY-PER-DEVICE (docs/lifecycle.md D2).
//!
//! A small file in the photon config dir (NOT the identity vault — it must be readable BEFORE any handle is typed, and the vault key-space derives from the handle) holding the party id of the identity this device is bound to, sealed under a key derived from the DEVICE secret (deterministic from the machine fingerprint, so it survives restarts and needs no stored key). The Launch probe consults it with the CHEAP party-id derivation of the typed handle — a mismatch refuses before the ~1s memory-hard proof is ever spent. Written on attest/join success; cleared only by a wipe (clean_device_for_reuse) — a takeover-cleared SESSION does not unbind a device, only wiping does. The worker's one-owner-per-device index is the backstop for a scrubbed marker.

const MARKER_FILE: &str = "device_binding.vsf";
const KEY_DOMAIN: &str = "photon.device_binding.v0";

fn marker_path() -> Option<std::path::PathBuf> {
    crate::storage::photon_config_dir().ok().map(|d| d.join(MARKER_FILE))
}

fn seal_key(device_secret: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key(KEY_DOMAIN, device_secret)
}

/// The party id this device is bound to, or `None` (unbound / unreadable / wrong device key — all read as unbound; the worker index backstops).
pub fn bound_party_id(device_secret: &[u8; 32]) -> Option<[u8; 32]> {
    let bytes = std::fs::read(marker_path()?).ok()?;
    let plain = kete::decrypt_bytes(&bytes, &seal_key(device_secret)).ok()?;
    plain.as_slice().try_into().ok()
}

/// Bind this device to `party_id`. Best-effort — a failed write only weakens the EARLY gate; the worker index still enforces.
pub fn bind(device_secret: &[u8; 32], party_id: &[u8; 32]) {
    let Some(path) = marker_path() else { return };
    match kete::encrypt_bytes(party_id.as_slice(), &seal_key(device_secret)) {
        Ok(sealed) => {
            if let Err(e) = std::fs::write(&path, sealed) {
                crate::logf!("BINDING: marker write failed: {}", e);
            }
        }
        Err(e) => crate::logf!("BINDING: marker seal failed: {}", e),
    }
}

/// Unbind (wipe path). Removing a missing file is the goal state, not an error.
pub fn clear() {
    if let Some(path) = marker_path() {
        let _ = std::fs::remove_file(path);
    }
}
