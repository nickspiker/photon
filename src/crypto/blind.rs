//! Friend-blinded storage of the private identity secret S.
//!
//! S is a 32-byte CSPRNG secret, generated once per identity, NEVER persisted anywhere — RAM-only (`Zeroizing`), reconstituted on demand from friends. Each of our devices deposits with each mutual friend a BLIND: `S ⊕ pad(device, friend)`, where the pad is a spaghettify derivation over the device secret and the friend's identity seed. The pad is full-entropy 32 bytes, so the blind is a ONE-TIME PAD ciphertext — the friend provably learns nothing about S from holding it (information-theoretic, not computational). Only a device that can re-derive the pad (fingerprint-deterministic device secret — survives a vault nuke) can unblind, and friends serve blinds only to devices they already trust for us (`knows_device`), so possession of the fleet is the key.
//!
//! Blob layout (64 bytes): `(S ⊕ pad) ‖ check`, where `check = spaghettify(domain ‖ S)`. Raw XOR is malleable — a bit-flipped blind would silently reconstruct S⊕Δ — so the check makes tampering (and a wrong pad) detectable, and doubles as the free cross-friend consistency test: blinds served by two friends either reconstruct the SAME S or fail loudly. The check is a one-way commitment to a 256-bit CSPRNG secret; it gives a friend no brute-force surface.
//!
//! `s_id` (4 bytes) is the tag EPOCH: a public fingerprint of S carried in history-authentication tags so that a regenerated S (all blinds lost) degrades old tags to "unverified" instead of hard-mismatching them.
//!
//! Derivation idiom matches `crypto::clutch` (domain const ‖ inputs → spaghettify, inputs zeroized); see `derive_history_key`.

use ihi::spaghettify;
use zeroize::{Zeroize, Zeroizing};

/// The private identity secret S, RAM-only — NEVER persisted (the whole point: at-rest copies only add theft surface; S is needed exactly when a friend is online, and then a friend can serve the blind back).
///
/// `None` → no S this session (probe friends before generating). `Provisional` → freshly generated, no friend has disk-confirmed a deposit yet — a crash here orphans nothing because Provisional S never tags anything. `Live` → at least one `blind_ack` landed; `s_id` is the public tag epoch.
#[derive(Default)]
pub enum PrivateS {
    #[default]
    None,
    Provisional(Zeroizing<[u8; 32]>),
    Live {
        s: Zeroizing<[u8; 32]>,
        s_id: [u8; 4],
    },
}

impl PrivateS {
    /// The secret bytes, whatever the state (None → None).
    pub fn secret(&self) -> Option<&Zeroizing<[u8; 32]>> {
        match self {
            PrivateS::None => None,
            PrivateS::Provisional(s) => Some(s),
            PrivateS::Live { s, .. } => Some(s),
        }
    }

    /// Live secret + epoch — the only state that may author tags.
    pub fn live(&self) -> Option<(&Zeroizing<[u8; 32]>, [u8; 4])> {
        match self {
            PrivateS::Live { s, s_id } => Some((s, *s_id)),
            _ => None,
        }
    }
}

/// Domain separation for the per-(device, friend) blind pad.
const BLIND_PAD_DOMAIN: &[u8] = b"PHOTON_BLIND_PAD_v0";
/// Domain separation for the tamper-detection check appended to the blind.
const BLIND_CHECK_DOMAIN: &[u8] = b"PHOTON_BLIND_CHECK_v0";
/// Domain separation for the tag-epoch fingerprint of S.
const S_ID_DOMAIN: &[u8] = b"PHOTON_S_ID_v0";

/// The wire/storage size of a blind blob: 32-byte OTP ciphertext + 32-byte check.
pub const BLIND_BLOB_LEN: usize = 64;

/// Derive the blind pad for (this device, that friend): `spaghettify(domain ‖ device_secret ‖ friend_pin)`.
///
/// Per-device AND per-friend: colluding friends can't even correlate their blinds (each is an independent OTP), and a stolen device's pad unblinds nothing once friends refuse it the serve. The friend context is their pinned identity PUBKEY (the contact party id — stable, rename-proof, and holdable without signing power; was their identity seed before the pin-set conversion, docs/identity-profile.md). The pad's secrecy comes entirely from `device_secret`; the context only needs stability + per-friend uniqueness. Recomputable any session from the fingerprint-deterministic device secret — nothing about the pad is ever stored.
pub fn derive_blind_pad(device_secret: &[u8; 32], friend_seed: &[u8; 32]) -> [u8; 32] {
    let mut input = Vec::with_capacity(BLIND_PAD_DOMAIN.len() + 64);
    input.extend_from_slice(BLIND_PAD_DOMAIN);
    input.extend_from_slice(device_secret);
    input.extend_from_slice(friend_seed);
    let pad = spaghettify(&input);
    // The input buffer holds the device secret — scrub it.
    input.zeroize();
    pad
}

/// The tamper-detection check for S: `spaghettify(domain ‖ S)`. One-way commitment; identical for every blind of the same S, which is what makes cross-friend consistency checking free.
pub fn s_check(s: &[u8; 32]) -> [u8; 32] {
    let mut input = Vec::with_capacity(BLIND_CHECK_DOMAIN.len() + 32);
    input.extend_from_slice(BLIND_CHECK_DOMAIN);
    input.extend_from_slice(s);
    let check = spaghettify(&input);
    input.zeroize();
    check
}

/// The 4-byte tag epoch of S. Public — it rides every history tag so a regenerated S is recognisable as a different epoch (old tags verify as "unverified", never as a false mismatch).
pub fn s_id(s: &[u8; 32]) -> [u8; 4] {
    let mut input = Vec::with_capacity(S_ID_DOMAIN.len() + 32);
    input.extend_from_slice(S_ID_DOMAIN);
    input.extend_from_slice(s);
    let full = spaghettify(&input);
    input.zeroize();
    [full[0], full[1], full[2], full[3]]
}

/// Seal S into the 64-byte blind blob a friend stores: `(S ⊕ pad) ‖ check(S)`.
pub fn make_blind_blob(s: &[u8; 32], pad: &[u8; 32]) -> [u8; BLIND_BLOB_LEN] {
    let mut blob = [0u8; BLIND_BLOB_LEN];
    for i in 0..32 {
        blob[i] = s[i] ^ pad[i];
    }
    blob[32..].copy_from_slice(&s_check(s));
    blob
}

/// Unblind a served blob with this device's pad and verify the check. `None` on any mismatch — wrong pad (not our deposit), tampered blind, tampered check, or a short/oversized blob. The caller tries the next friend.
pub fn open_blind_blob(blob: &[u8], pad: &[u8; 32]) -> Option<Zeroizing<[u8; 32]>> {
    if blob.len() != BLIND_BLOB_LEN {
        return None;
    }
    let mut s = Zeroizing::new([0u8; 32]);
    for i in 0..32 {
        s[i] = blob[i] ^ pad[i];
    }
    if s_check(&s) != blob[32..] {
        return None;
    }
    Some(s)
}

/// Seal S for transfer to a FLEET SIBLING: kete ChaCha20-Poly1305 under the sibling chains' history key (spaghettify-derived at ceremony birth, known only to the two devices). Unlike friend blinds this is a computational seal, not an OTP — acceptable because the recipient is OUR OWN device and the key never leaves the pair. The plaintext carries the check so a wrong-key or corrupted open fails closed. NEVER rides chat messages: chain messages persist to the conversation DB, and S at rest — even vault-encrypted — is exactly what never-at-rest forbids.
pub fn seal_sibling_s(s: &[u8; 32], chain_key: &[u8; 32]) -> Result<Vec<u8>, String> {
    let mut plain = Vec::with_capacity(64);
    plain.extend_from_slice(s);
    plain.extend_from_slice(&s_check(s));
    let sealed = kete::encrypt_bytes(&plain, chain_key).map_err(|e| e.to_string());
    plain.zeroize();
    sealed
}

/// Open a sibling-sealed S. `None` on AEAD failure, wrong length, or check mismatch.
pub fn open_sibling_s(sealed: &[u8], chain_key: &[u8; 32]) -> Option<Zeroizing<[u8; 32]>> {
    let mut plain = kete::decrypt_bytes(sealed, chain_key).ok()?;
    if plain.len() != 64 {
        plain.zeroize();
        return None;
    }
    let mut s = Zeroizing::new([0u8; 32]);
    s.copy_from_slice(&plain[..32]);
    let ok = s_check(&s) == plain[32..];
    plain.zeroize();
    if ok {
        Some(s)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_deterministic_and_distinct_per_device_and_friend() {
        let dev_a = [1u8; 32];
        let dev_b = [2u8; 32];
        let friend_x = [9u8; 32];
        let friend_y = [10u8; 32];
        assert_eq!(
            derive_blind_pad(&dev_a, &friend_x),
            derive_blind_pad(&dev_a, &friend_x),
            "same (device, friend) → same pad, any session"
        );
        assert_ne!(derive_blind_pad(&dev_a, &friend_x), derive_blind_pad(&dev_b, &friend_x));
        assert_ne!(derive_blind_pad(&dev_a, &friend_x), derive_blind_pad(&dev_a, &friend_y));
    }

    #[test]
    fn blind_round_trip_recovers_s() {
        let s = [0x5Au8; 32];
        let pad = derive_blind_pad(&[1u8; 32], &[9u8; 32]);
        let blob = make_blind_blob(&s, &pad);
        let opened = open_blind_blob(&blob, &pad).expect("check must pass");
        assert_eq!(*opened, s);
    }

    #[test]
    fn tamper_and_wrong_pad_all_fail_closed() {
        let s = [0x5Au8; 32];
        let pad = derive_blind_pad(&[1u8; 32], &[9u8; 32]);
        let blob = make_blind_blob(&s, &pad);

        // Flipped ciphertext bit → S⊕Δ fails the check (the malleability defense).
        let mut t1 = blob;
        t1[7] ^= 0x01;
        assert!(open_blind_blob(&t1, &pad).is_none());

        // Flipped check bit → fails.
        let mut t2 = blob;
        t2[40] ^= 0x80;
        assert!(open_blind_blob(&t2, &pad).is_none());

        // Wrong pad (another device's deposit) → fails, caller tries elsewhere.
        let other_pad = derive_blind_pad(&[2u8; 32], &[9u8; 32]);
        assert!(open_blind_blob(&blob, &other_pad).is_none());

        // Wrong length → fails.
        assert!(open_blind_blob(&blob[..63], &pad).is_none());
        assert!(open_blind_blob(&[0u8; 65], &pad).is_none());
    }

    #[test]
    fn sibling_seal_round_trip_and_tamper() {
        let s = [0x5Au8; 32];
        let key = [0x77u8; 32];
        let sealed = seal_sibling_s(&s, &key).unwrap();
        let opened = open_sibling_s(&sealed, &key).expect("AEAD + check must pass");
        assert_eq!(*opened, s);
        assert!(open_sibling_s(&sealed, &[0x78u8; 32]).is_none(), "wrong key fails closed");
        let mut t = sealed.clone();
        let mid = t.len() / 2;
        t[mid] ^= 0x01;
        assert!(open_sibling_s(&t, &key).is_none(), "tampered ciphertext fails closed");
    }

    #[test]
    fn s_id_and_check_are_domain_separated() {
        let s = [0x33u8; 32];
        // The epoch is the first 4 bytes of a DIFFERENT derivation than the check — the check must not leak thru the public epoch.
        assert_ne!(s_id(&s), s_check(&s)[..4]);
        // Distinct secrets → distinct epochs (probabilistically; exact inputs here).
        assert_ne!(s_id(&s), s_id(&[0x34u8; 32]));
    }
}
