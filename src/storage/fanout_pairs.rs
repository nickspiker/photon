//! Durable per-device-pair secrets for the egged fan-out (Phase A).
//!
//! Each completed CLUTCH ceremony between two of OUR fleet devices mints a pair secret (`crypto::clutch::derive_fanout_pair_secret`) carrying all 8 KEM families. The fan-out then keys each wrap with the fresh x25519 ECDH AND this secret, so a harvested blob is not opened by a future x25519 break.
//! A device with no stored secret toward the rotator is NOT COMPLIANT: it receives no wrap and stays dark until its pair ceremony completes and the next rotation includes it (hard flag-day, user directive 2026-08-01).
//!
//! Storage is one vault entry per pair, addressed by the sorted-pair hash — so either device computes the same address, and a pair survives independently of any other.

use crate::storage::{FlatStorage, StorageError};

// Version as a literal binary numeral, never an ASCII digit welded into the string (repo convention 2026-08-01).
const PAIR_ADDR_DOMAIN_TEXT: &[u8] = b"PHOTON_FANOUT_PAIR_ADDR_v";
const PAIR_ADDR_VERSION: u8 = 0;

/// The vault address for one device pair. Sorted, so both devices agree without negotiating.
fn pair_addr(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let mut h = blake3::Hasher::new();
    h.update(PAIR_ADDR_DOMAIN_TEXT);
    h.update(&[PAIR_ADDR_VERSION]);
    h.update(lo);
    h.update(hi);
    let scope: [u8; 32] = *h.finalize().as_bytes();
    crate::storage::vault_key("fanout_pair", &scope)
}

/// Store the pair secret for (`ours`, `theirs`). Idempotent — a re-clutch overwrites with the fresh ceremony's secret, which is exactly what the next rotation should wrap under.
pub fn store(
    ours: &[u8; 32],
    theirs: &[u8; 32],
    secret: &[u8; 32],
    storage: &FlatStorage,
) -> Result<(), StorageError> {
    storage.write_addr(&pair_addr(ours, theirs), secret)
}

/// Load the pair secret for (`ours`, `theirs`), or `None` when this pair has never completed a ceremony — the non-compliant case.
pub fn load(ours: &[u8; 32], theirs: &[u8; 32], storage: &FlatStorage) -> Option<[u8; 32]> {
    let bytes = storage.read_addr(&pair_addr(ours, theirs)).ok()??;
    <[u8; 32]>::try_from(bytes.as_slice()).ok()
}

/// The SELF pair: a rotator must wrap its own copy too, and there is no ceremony with oneself. Derived from the device's own key material rather than stored, so it exists from first launch and never needs a ceremony.
pub fn self_secret(device_secret: &[u8; 32], device_pubkey: &[u8; 32]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(b"PHOTON_FANOUT_SELF_PAIR_v");
    h.update(&[PAIR_ADDR_VERSION]);
    h.update(device_secret);
    h.update(device_pubkey);
    *h.finalize().as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both devices must land on the SAME vault address regardless of which one asks — otherwise each side would store a secret the other can't find and every wrap would be non-compliant.
    #[test]
    fn pair_address_is_order_independent() {
        let a = [7u8; 32];
        let b = [9u8; 32];
        assert_eq!(pair_addr(&a, &b), pair_addr(&b, &a));
        // A different pair is a different address (no collision across the fleet).
        assert_ne!(pair_addr(&a, &b), pair_addr(&a, &[11u8; 32]));
    }

    /// The self secret is stable for a device and distinct from any other device's — it is the rotator's own wrap key half.
    #[test]
    fn self_secret_is_stable_and_device_bound() {
        let sk = [3u8; 32];
        let pk = [4u8; 32];
        assert_eq!(self_secret(&sk, &pk), self_secret(&sk, &pk));
        assert_ne!(self_secret(&sk, &pk), self_secret(&[5u8; 32], &pk));
    }
}
