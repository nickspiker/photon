//! The device-binding marker — the client half of ONE-IDENTITY-PER-DEVICE (docs/lifecycle.md D2).
//!
//! A DEVICE-scope vault entry (readable before any handle is typed — the device vault opens from the device secret alone at first launch) naming the identity this device is bound to. The v1 marker holds a keyed blake3 digest of the MEMORY-HARD handle proof, so the busy compare can only run AFTER the ~1s hardening and every wrong-handle guess costs a full proof — the marker at rest verifies nothing cheaply (the 2026-08-23 oracle ticket: the v0 party-id marker let anyone holding the device sweep candidate handles at microseconds each, and handles are SSN-grade). The digest is derive-keyed rather than the raw proof because the proof is the public FGTW slot key — a raw copy would link the device vault to the public chain slot for free. v0 markers (the raw cheap party id) still compare once for continuity and are rewritten as v1 by the next successful attest — the same self-heal channel that backfilled the marker on legacy devices.
//! Written on attest/join success; cleared only by a wipe (clean_device_for_reuse) — a takeover-cleared SESSION does not unbind a device, only wiping does. The worker's one-owner-per-device index is the backstop for a scrubbed marker.
//! Was a loose sealed file (`device_binding.vsf`) in the file-sprawl era; that file is a census stray now (deleted, not imported).

const ENTRY: &str = "binding/party";
const V1_TAG: u8 = 1;
const V1_CTX: &str = "photon.device_binding.proof.v1";

/// What the marker holds: v1 = hardened proof digest; v0 = the legacy cheap party id, honoured until the next successful attest upgrades it.
pub enum Binding {
    ProofKey([u8; 32]),
    LegacyPid([u8; 32]),
}

/// Parse a marker value. Length discriminates: 33 bytes under the v1 tag = proof digest, 32 bytes = legacy pid (a pid whose first byte happens to be the tag is still 32 bytes, so it can never misread as v1), anything else = unbound.
fn parse(bytes: &[u8]) -> Option<Binding> {
    if bytes.len() == 33 && bytes[0] == V1_TAG {
        bytes[1..].try_into().ok().map(Binding::ProofKey)
    } else {
        bytes.try_into().ok().map(Binding::LegacyPid)
    }
}

/// The marker, or `None` (unbound / vault unavailable / unparseable — reads as unbound; the worker index backstops).
pub fn binding() -> Option<Binding> {
    let vault = crate::storage::device_vault()?;
    let bytes = vault.read_device(ENTRY).ok()??;
    parse(&bytes)
}

fn proof_key(handle_proof: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key(V1_CTX, handle_proof)
}

/// The DEVICE BUSY verdict: a marker exists and names a DIFFERENT identity than (handle_proof, party_id). Call only AFTER the memory-hard proof is in hand — that ordering is the whole fix; a call site that wants to compare before the proof is the oracle coming back.
pub fn busy_for(handle_proof: &[u8; 32], party_id: &[u8; 32]) -> bool {
    match binding() {
        Some(Binding::ProofKey(k)) => k != proof_key(handle_proof),
        Some(Binding::LegacyPid(pid)) => pid != *party_id,
        None => false,
    }
}

/// Bind this device to the attested identity — writes the v1 hardened marker (upgrading any v0 pid in place). Best-effort — a failed write only weakens the EARLY gate; the worker index still enforces.
pub fn bind(handle_proof: &[u8; 32]) {
    let Some(vault) = crate::storage::device_vault() else {
        return;
    };
    let mut value = [0u8; 33];
    value[0] = V1_TAG;
    value[1..].copy_from_slice(&proof_key(handle_proof));
    if let Err(e) = vault.write_device(ENTRY, &value) {
        crate::logf!("BINDING: marker write failed: {}", e);
    }
}

/// Unbind (wipe path). Deleting an absent entry is the goal state, not an error.
pub fn clear() {
    if let Some(vault) = crate::storage::device_vault() {
        let _ = vault.delete_device(ENTRY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_format_discriminates_by_length_and_tag() {
        let proof = [7u8; 32];
        let mut v1 = [0u8; 33];
        v1[0] = V1_TAG;
        v1[1..].copy_from_slice(&proof_key(&proof));
        assert!(matches!(parse(&v1), Some(Binding::ProofKey(k)) if k == proof_key(&proof)));
        // A legacy pid stays a pid even when its first byte collides with the v1 tag — length is the discriminator.
        let mut pid = [0u8; 32];
        pid[0] = V1_TAG;
        assert!(matches!(parse(&pid), Some(Binding::LegacyPid(p)) if p == pid));
        assert!(parse(&[0u8; 16]).is_none());
        assert!(parse(&[]).is_none());
    }

    #[test]
    fn busy_verdict_matches_only_its_own_identity() {
        let proof = [3u8; 32];
        let other_proof = [4u8; 32];
        // v1 semantics via the pure halves (no vault in unit tests): the stored digest matches its own proof and refuses any other.
        assert_eq!(proof_key(&proof), proof_key(&proof));
        assert_ne!(proof_key(&proof), proof_key(&other_proof));
    }
}
