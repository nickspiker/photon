//! The device-binding marker — the client half of ONE-IDENTITY-PER-DEVICE (docs/lifecycle.md D2).
//!
//! A DEVICE-scope vault entry (readable before any handle is typed — the device vault opens from the device secret alone at first launch) holding the party id of the identity this device is bound to. The Launch probe consults it with the CHEAP party-id derivation of the typed handle — a mismatch refuses before the ~1s memory-hard proof is ever spent. Written on attest/join success; cleared only by a wipe (clean_device_for_reuse) — a takeover-cleared SESSION does not unbind a device, only wiping does. The worker's one-owner-per-device index is the backstop for a scrubbed marker.
//! Was a loose sealed file (`device_binding.vsf`) in the file-sprawl era; that file is a census stray now (deleted, not imported) — the worker's binding index backstops a device that loses the local marker.

const ENTRY: &str = "binding/party";

/// The party id this device is bound to, or `None` (unbound / vault unavailable — reads as unbound; the worker index backstops).
pub fn bound_party_id() -> Option<[u8; 32]> {
    let vault = crate::storage::device_vault()?;
    let bytes = vault.read_device(ENTRY).ok()??;
    bytes.as_slice().try_into().ok()
}

/// Bind this device to `party_id`. Best-effort — a failed write only weakens the EARLY gate; the worker index still enforces.
pub fn bind(party_id: &[u8; 32]) {
    let Some(vault) = crate::storage::device_vault() else { return };
    if let Err(e) = vault.write_device(ENTRY, party_id.as_slice()) {
        crate::logf!("BINDING: marker write failed: {}", e);
    }
}

/// Unbind (wipe path). Deleting an absent entry is the goal state, not an error.
pub fn clear() {
    if let Some(vault) = crate::storage::device_vault() {
        let _ = vault.delete_device(ENTRY);
    }
}
