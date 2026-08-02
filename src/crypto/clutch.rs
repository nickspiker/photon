use crate::types::Seed;
use blake3::Hasher;
use ihi::{smear_hash, spaghettify};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroize;

/// Domain separation for conversation token derivation
const CONVERSATION_TOKEN_DOMAIN: &[u8] = b"PHOTON_CONVERSATION_TOKEN_v\x01";

/// Derive a privacy-preserving conversation token from participant identity seeds.
///
/// Works for N-party conversations (2-party, 3-party, etc.). All participants derive the SAME token by sorting seeds lexicographically before hashing. The token:
/// - Only participants can compute (requires knowing all identity seeds)
/// - Doesn't reveal individual identities to network observers
/// - Different for each unique set of participants
///
/// Uses spaghettify for maximum obfuscation (domain-separated, maximally weird mixing). VSF type: hg (spaghetti hash)
pub fn derive_conversation_token(participant_seeds: &[[u8; 32]]) -> [u8; 32] {
    // Canonical ordering - ALL participants compute same token
    let mut sorted_seeds = participant_seeds.to_vec();
    sorted_seeds.sort(); // Lexicographic sort of 32-byte arrays

    // Domain separation + concatenated seeds
    let mut input = Vec::with_capacity(CONVERSATION_TOKEN_DOMAIN.len() + sorted_seeds.len() * 32);
    input.extend_from_slice(CONVERSATION_TOKEN_DOMAIN);
    for seed in &sorted_seeds {
        input.extend_from_slice(seed);
    }

    spaghettify(&input)
}

/// Domain separation for the friend-history bulk key
const HISTORY_KEY_DOMAIN: &[u8] = b"PHOTON_HISTORY_KEY_v\x01";

// The fan-out pair secret's domain. Version is a literal binary numeral, never an ASCII digit in the string (repo convention 2026-08-01).
const FANOUT_PAIR_DOMAIN_TEXT: &[u8] = b"PHOTON_FANOUT_PAIR_v";
const FANOUT_PAIR_VERSION: u8 = 0;

/// Derive the durable PAIR SECRET two fleet devices share after a CLUTCH ceremony: the post-quantum half of an egged fan-out wrap (Phase A). Symmetric by construction — device pubkeys are sorted — so both sides derive the same 32 bytes and either may be the rotator.
///
/// input = DOMAIN ‖ version ‖ sorted device pubkeys ‖ every egg, run thru spaghettify. The eggs carry all 8 KEM families, so a wrap keyed with this survives a future x25519 break; the fan-out still mixes the fresh ECDH alongside it, so this secret alone is not a skeleton key either. Derived at ceremony completion BEFORE the eggs are zeroized, then stored per pair.
pub fn derive_fanout_pair_secret(
    our_device_pubkey: &[u8; 32],
    their_device_pubkey: &[u8; 32],
    eggs: &ClutchEggs,
) -> [u8; 32] {
    let (lo, hi) = if our_device_pubkey <= their_device_pubkey {
        (our_device_pubkey, their_device_pubkey)
    } else {
        (their_device_pubkey, our_device_pubkey)
    };
    let mut input =
        Vec::with_capacity(FANOUT_PAIR_DOMAIN_TEXT.len() + 1 + 64 + eggs.eggs.len() * 32);
    input.extend_from_slice(FANOUT_PAIR_DOMAIN_TEXT);
    input.push(FANOUT_PAIR_VERSION);
    input.extend_from_slice(lo);
    input.extend_from_slice(hi);
    for egg in &eggs.eggs {
        input.extend_from_slice(egg);
    }
    let secret = spaghettify(&input);
    // The buffer holds live egg material — scrub it.
    input.zeroize();
    secret
}

/// Derive the friend-history bulk key: the AEAD key that seals history-recovery pages between the participants, OUTSIDE the ratchet (bulk backfill must not advance or pollute the live braid).
///
/// input = DOMAIN ‖ friendship_id ‖ each participant's ACTIVE chain half (8KB, links[256..512]) in participant-sorted order, run thru spaghettify (domain-separated, provably lossy — the key cannot be inverted back to chain material). Both sides hold byte-identical pristine chains at ceremony birth, so the key is identical; any later advance diverges — hence derive-at-birth only, in `FriendshipChains::from_clutch`. Superseded (and zeroized) on re-key.
pub fn derive_history_key(friendship_id: &[u8; 32], active_chains_sorted: &[&[u8]]) -> [u8; 32] {
    let total: usize = active_chains_sorted.iter().map(|c| c.len()).sum();
    let mut input = Vec::with_capacity(HISTORY_KEY_DOMAIN.len() + 32 + total);
    input.extend_from_slice(HISTORY_KEY_DOMAIN);
    input.extend_from_slice(friendship_id);
    for chain in active_chains_sorted {
        input.extend_from_slice(chain);
    }
    let key = spaghettify(&input);
    // The input buffer holds live chain secret material — scrub it.
    input.zeroize();
    key
}

/// Domain for the lane root (docs/lanes.md). Version is a binary numeral, per convention.
const LANE_ROOT_DOMAIN: &[u8] = b"PHOTON_LANE_ROOT_v\x01";

/// Derive the LANE ROOT: the 32-byte secret every per-device lane grows from (docs/lanes.md). Same birth moment and same construction discipline as [`derive_history_key`] — the pristine active chains are byte-identical on both sides exactly once, at ceremony completion, and spaghettify is provably lossy so the root cannot be inverted back to chain material. Device identity is deliberately NOT an input: lanes derive from root ‖ wire label alone, which is what makes receive-anywhere and label pseudonymity work. Superseded (and zeroized) on re-key.
pub fn derive_lane_root(friendship_id: &[u8; 32], active_chains_sorted: &[&[u8]]) -> [u8; 32] {
    let total: usize = active_chains_sorted.iter().map(|c| c.len()).sum();
    let mut input = Vec::with_capacity(LANE_ROOT_DOMAIN.len() + 32 + total);
    input.extend_from_slice(LANE_ROOT_DOMAIN);
    input.extend_from_slice(friendship_id);
    for chain in active_chains_sorted {
        input.extend_from_slice(chain);
    }
    let key = spaghettify(&input);
    // The input buffer holds live chain secret material — scrub it.
    input.zeroize();
    key
}

/// OUR OWN party id as a FRIEND sees it: the Ed25519 identity pubkey derived from the identity seed — the same value a contact pins at first-met, so both sides sort/slot/derive on identical ids. Public by design (it rides CLUTCH offers for contact matching); the SECRET identity binding moved to [`identity_friendship_secret`]. Supersedes using the raw identity seed as the party id, which parked the friend's SIGNING SEED in every contact row (docs/identity-profile.md).
pub fn identity_party_id(identity_seed: &[u8; 32]) -> [u8; 32] {
    ed25519_dalek::SigningKey::from_bytes(identity_seed)
        .verifying_key()
        .to_bytes()
}

/// The static identity Diffie-Hellman secret for a FRIEND ceremony: x25519 between OUR identity scalar and THEIR pinned identity pubkey's Montgomery form — computable by exactly the two identity holders, from the pin-set alone, no wire exchange. Same Ed25519→X25519 construction as the fgtw fan-out (`to_scalar_bytes` / `to_montgomery` agree on the same point), hashed under a domain so the raw DH point never leaves this function. `None` when the pinned bytes don't decode as a curve point; an old-format row that happens to decode anyway just derives a secret the peer won't match, failing the ceremony at proof verification — the same flag-day outcome, one step later. Fleet siblings don't DH (their party ids aren't curve points): both devices share the identity seed itself, so the caller passes that instead.
pub fn identity_friendship_secret(
    our_identity_seed: &[u8; 32],
    their_identity_pubkey: &[u8; 32],
) -> Option<[u8; 32]> {
    let their_vk = ed25519_dalek::VerifyingKey::from_bytes(their_identity_pubkey).ok()?;
    let our_x = StaticSecret::from(
        ed25519_dalek::SigningKey::from_bytes(our_identity_seed).to_scalar_bytes(),
    );
    let shared = our_x.diffie_hellman(&PublicKey::from(their_vk.to_montgomery().to_bytes()));
    let mut hasher = Hasher::new();
    hasher.update(b"PHOTON_FRIENDSHIP_DH_v1");
    hasher.update(shared.as_bytes());
    Some(*hasher.finalize().as_bytes())
}

/// Domain separation for sibling (own-fleet device) party ids
const SIBLING_PARTY_DOMAIN: &[u8] = b"PHOTON_SIBLING_PARTY_v\x01";

/// Derive the CLUTCH party id for a fleet sibling device.
///
/// Every derivation in the ceremony/braid stack (PartySlot keying, CeremonyId, FriendshipId, conversation_token, chain indices) operates on opaque sorted 32-byte party ids — for friends that id is the handle_hash. Two devices of the SAME user share one handle, so handle_hash collides at every layer; sibling ceremonies key on this device-derived id instead. The domain prefix separates the id space from handle_hash (BLAKE3 of a VSF x-typed handle), so a sibling pid can never collide with any friend's handle_hash. Device pubkeys are already public in the fleet membership chain, and party ids only reach the wire spaghettified (conversation_token, ceremony_id), so BLAKE3 suffices here.
pub fn sibling_party_id(device_pubkey: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(SIBLING_PARTY_DOMAIN);
    hasher.update(device_pubkey);
    *hasher.finalize().as_bytes()
}

#[cfg(test)]
mod identity_binding_tests {
    use super::*;

    #[test]
    fn friendship_secret_is_symmetric_and_pairwise_unique() {
        let seed_a = [1u8; 32];
        let seed_b = [2u8; 32];
        let seed_c = [3u8; 32];
        let (pk_a, pk_b, pk_c) = (
            identity_party_id(&seed_a),
            identity_party_id(&seed_b),
            identity_party_id(&seed_c),
        );
        // Both identity holders compute the SAME secret from opposite ends — the static DH that replaced mutual-handle-knowledge.
        let ab = identity_friendship_secret(&seed_a, &pk_b).unwrap();
        let ba = identity_friendship_secret(&seed_b, &pk_a).unwrap();
        assert_eq!(ab, ba);
        // Distinct per pair, and never trivially related to the party ids.
        let ac = identity_friendship_secret(&seed_a, &pk_c).unwrap();
        assert_ne!(ab, ac);
        assert_ne!(ab, pk_a);
        assert_ne!(ab, pk_b);
        // (A garbage pin often still decodes as SOME point — the mismatch then surfaces as a CLUTCH proof failure, the same flag-day outcome as an outright None.) Party id is deterministic and distinct from the seed it derives from.
        assert_eq!(pk_a, identity_party_id(&seed_a));
        assert_ne!(pk_a, seed_a);
    }
}

/// Domain separator for ceremony instance derivation
const CEREMONY_INSTANCE_DOMAIN: &[u8] = b"PHOTON_CEREMONY_INSTANCE_v\x01";

/// Derive a unique ceremony instance identifier from all parties' offers.
///
/// This is used for stale detection: distinguishes re-key requests from PT retransmissions. Unlike ceremony_id (derived from handle_hashes, invariant per handle pair), this changes when ephemeral keypairs are regenerated.
///
/// Both parties can compute this independently once they have both offers. Works for N-party ceremonies.
pub fn derive_ceremony_instance(offers: &[&ClutchOfferPayload]) -> [u8; 32] {
    // Serialize each offer to bytes (concatenate all 8 public keys)
    let mut offer_bytes: Vec<Vec<u8>> = offers.iter().map(|o| o.to_bytes()).collect();

    // Canonical ordering - sort by serialized bytes
    offer_bytes.sort();

    // Domain separation + concatenated sorted offers
    let mut input = Vec::with_capacity(
        CEREMONY_INSTANCE_DOMAIN.len() + offer_bytes.iter().map(|b| b.len()).sum::<usize>(),
    );
    input.extend_from_slice(CEREMONY_INSTANCE_DOMAIN);
    for bytes in &offer_bytes {
        input.extend_from_slice(bytes);
    }

    smear_hash(&input)
}

/// Determine who initiates clutch ceremony. Lower handle_proof = initiator (sends ephemeral pubkeys first) Higher handle_proof = responder (waits, then responds)
///
/// All parties compute the same result from sorted handle hashes.
pub fn is_clutch_initiator(local_handle_proof: &[u8; 32], remote_handle_proof: &[u8; 32]) -> bool {
    local_handle_proof < remote_handle_proof
}

/// Generate ephemeral X25519 keypair Returns (secret, public) - caller MUST zeroize the secret after use!
pub fn generate_x25519_ephemeral() -> ([u8; 32], [u8; 32]) {
    let mut secret_bytes = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut secret_bytes);

    let secret = StaticSecret::from(secret_bytes);
    let public = PublicKey::from(&secret);

    // Return the secret bytes for the caller to use (and zeroize when done) Note: StaticSecret::from() copies the bytes, so we return the original
    (secret_bytes, *public.as_bytes())
}

/// Perform X25519 ECDH to derive shared secret. Caller should zeroize the returned shared secret after use.
pub fn x25519_ecdh(local_secret: &[u8; 32], peer_public: &[u8; 32]) -> [u8; 32] {
    let secret = StaticSecret::from(*local_secret);
    let public = PublicKey::from(*peer_public);
    let shared = secret.diffie_hellman(&public);
    // x25519_dalek's SharedSecret zeroizes on drop, but we need the bytes
    *shared.as_bytes()
}

// ============================================================================ CLASS 0: CLASSICAL ELLIPTIC CURVES ============================================================================

/// Generate P-384 ephemeral keypair Returns (secret_bytes, public_bytes)
pub fn generate_p384_ephemeral() -> (Vec<u8>, Vec<u8>) {
    use p384::elliptic_curve::Generate;
    use p384::SecretKey;

    let secret = SecretKey::generate();
    let public = secret.public_key();

    let secret_bytes = secret.to_bytes().to_vec();
    let public_bytes = public.to_sec1_bytes().to_vec();

    (secret_bytes, public_bytes)
}

/// Perform P-384 ECDH. Returns 48-byte shared secret.
pub fn p384_ecdh(local_secret: &[u8], peer_public: &[u8]) -> Option<Vec<u8>> {
    use p384::elliptic_curve::ecdh::diffie_hellman;
    use p384::{PublicKey, SecretKey};

    let secret = SecretKey::from_slice(local_secret).ok()?;
    let public = PublicKey::from_sec1_bytes(peer_public).ok()?;

    let shared = diffie_hellman(secret.to_nonzero_scalar(), public.as_affine());
    Some(shared.raw_secret_bytes().to_vec())
}

/// Generate secp256k1 ephemeral keypair Returns (secret_bytes, public_bytes)
pub fn generate_secp256k1_ephemeral() -> (Vec<u8>, Vec<u8>) {
    use k256::elliptic_curve::Generate;
    use k256::SecretKey;

    let secret = SecretKey::generate();
    let public = secret.public_key();

    let secret_bytes = secret.to_bytes().to_vec();
    let public_bytes = public.to_sec1_bytes().to_vec();

    (secret_bytes, public_bytes)
}

/// Perform secp256k1 ECDH. Returns 32-byte shared secret.
pub fn secp256k1_ecdh(local_secret: &[u8], peer_public: &[u8]) -> Option<Vec<u8>> {
    use k256::elliptic_curve::ecdh::diffie_hellman;
    use k256::{PublicKey, SecretKey};

    let secret = SecretKey::from_slice(local_secret).ok()?;
    let public = PublicKey::from_sec1_bytes(peer_public).ok()?;

    let shared = diffie_hellman(secret.to_nonzero_scalar(), public.as_affine());
    Some(shared.raw_secret_bytes().to_vec())
}

/// Generate P-256 ephemeral keypair Returns (secret_bytes, public_bytes)
pub fn generate_p256_ephemeral() -> (Vec<u8>, Vec<u8>) {
    use p256::elliptic_curve::Generate;
    use p256::SecretKey;

    let secret = SecretKey::generate();
    let public = secret.public_key();

    let secret_bytes = secret.to_bytes().to_vec();
    let public_bytes = public.to_sec1_bytes().to_vec();

    (secret_bytes, public_bytes)
}

/// Perform P-256 ECDH. Returns 32-byte shared secret.
pub fn p256_ecdh(local_secret: &[u8], peer_public: &[u8]) -> Option<Vec<u8>> {
    use p256::elliptic_curve::ecdh::diffie_hellman;
    use p256::{PublicKey, SecretKey};

    let secret = SecretKey::from_slice(local_secret).ok()?;
    let public = PublicKey::from_sec1_bytes(peer_public).ok()?;

    let shared = diffie_hellman(secret.to_nonzero_scalar(), public.as_affine());
    Some(shared.raw_secret_bytes().to_vec())
}

/// Generate P-521 ephemeral keypair Returns (secret_bytes, public_bytes)
pub fn generate_p521_ephemeral() -> (Vec<u8>, Vec<u8>) {
    use p521::elliptic_curve::Generate;
    use p521::SecretKey;

    let secret = SecretKey::generate();
    let public = secret.public_key();

    (secret.to_bytes().to_vec(), public.to_sec1_bytes().to_vec())
}

/// Perform P-521 ECDH. Returns the raw shared secret (66 bytes at this curve size — every egg is hashed to 32 downstream, so width never matters here).
pub fn p521_ecdh(local_secret: &[u8], peer_public: &[u8]) -> Option<Vec<u8>> {
    use p521::elliptic_curve::ecdh::diffie_hellman;
    use p521::{PublicKey, SecretKey};

    let secret = SecretKey::from_slice(local_secret).ok()?;
    let public = PublicKey::from_sec1_bytes(peer_public).ok()?;

    let shared = diffie_hellman(secret.to_nonzero_scalar(), public.as_affine());
    Some(shared.raw_secret_bytes().to_vec())
}

// ============================================================================ CLASS 1: POST-QUANTUM LATTICE KEMS ============================================================================

/// Generate FrodoKEM-976-SHAKE keypair Returns (secret_key, public_key)
pub fn generate_frodo976_keypair() -> (Vec<u8>, Vec<u8>) {
    use frodo_kem_rs::Algorithm;
    use rand_core::OsRng;

    let alg = Algorithm::FrodoKem976Shake;
    let (ek, dk) = alg
        .try_generate_keypair(OsRng)
        .expect("FrodoKEM keygen failed");

    (dk.value().to_vec(), ek.value().to_vec())
}

/// Encapsulate FrodoKEM-976-SHAKE Returns (ciphertext, shared_secret)
pub fn frodo976_encapsulate(their_public_key: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    use frodo_kem_rs::{Algorithm, EncryptionKey};
    use rand_core::OsRng;

    let alg = Algorithm::FrodoKem976Shake;
    let ek =
        EncryptionKey::from_bytes(alg, their_public_key).ok()?;
    let (ct, ss) = alg
        .try_encapsulate_with_rng(&ek, OsRng)
        .ok()?;

    Some((ct.value().to_vec(), ss.value().to_vec()))
}

/// Decapsulate FrodoKEM-976-SHAKE Returns shared_secret
pub fn frodo976_decapsulate(our_secret_key: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    use frodo_kem_rs::{Algorithm, Ciphertext, DecryptionKey};

    let alg = Algorithm::FrodoKem976Shake;
    let dk = DecryptionKey::from_bytes(alg, our_secret_key).ok()?;
    let ct = Ciphertext::from_bytes(alg, ciphertext).ok()?;
    let (ss, _msg) = alg
        .decapsulate(&dk, &ct)
        .ok()?;

    Some(ss.value().to_vec())
}

/// Generate NTRU-HRSS-701 keypair Returns (secret_key, public_key)
pub fn generate_ntru701_keypair() -> (Vec<u8>, Vec<u8>) {
    use pqcrypto_ntru::ntruhrss701;
    use pqcrypto_traits::kem::{PublicKey, SecretKey};

    // NTRU uses its own internal RNG (PQClean's randombytes)
    let (pk, sk) = ntruhrss701::keypair();

    (
        SecretKey::as_bytes(&sk).to_vec(),
        PublicKey::as_bytes(&pk).to_vec(),
    )
}

/// Encapsulate NTRU-HRSS-701 Returns (ciphertext, 32B shared_secret)
pub fn ntru701_encapsulate(their_public_key: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    use pqcrypto_ntru::ntruhrss701;
    use pqcrypto_traits::kem::{Ciphertext, PublicKey, SharedSecret};

    let pk = <ntruhrss701::PublicKey as PublicKey>::from_bytes(their_public_key)
        .ok()?;

    // NTRU uses its own internal RNG for encapsulation
    let (ss, ct) = ntruhrss701::encapsulate(&pk);

    Some((
        Ciphertext::as_bytes(&ct).to_vec(),
        SharedSecret::as_bytes(&ss).to_vec(),
    ))
}

/// Decapsulate NTRU-HRSS-701 Returns 32B shared_secret
pub fn ntru701_decapsulate(our_secret_key: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    use pqcrypto_ntru::ntruhrss701;
    use pqcrypto_traits::kem::{Ciphertext, SecretKey, SharedSecret};

    let sk = <ntruhrss701::SecretKey as SecretKey>::from_bytes(our_secret_key)
        .ok()?;
    let ct = <ntruhrss701::Ciphertext as Ciphertext>::from_bytes(ciphertext)
        .ok()?;

    let ss = ntruhrss701::decapsulate(&ct, &sk);

    Some(SharedSecret::as_bytes(&ss).to_vec())
}

/// Generate FrodoKEM-1344-SHAKE keypair Returns (secret_key, public_key). The UNSTRUCTURED-lattice member at its largest parameter: plain LWE, no ring or module structure at all, so a structural break against NTRU/ML-KEM leaves it standing.
pub fn generate_frodo1344_keypair() -> (Vec<u8>, Vec<u8>) {
    use frodo_kem_rs::Algorithm;
    use rand_core::OsRng;

    let alg = Algorithm::FrodoKem1344Shake;
    let (ek, dk) = alg
        .try_generate_keypair(OsRng)
        .expect("FrodoKEM-1344 keygen failed");

    (dk.value().to_vec(), ek.value().to_vec())
}

/// Encapsulate FrodoKEM-1344-SHAKE Returns (ciphertext, shared_secret)
pub fn frodo1344_encapsulate(their_public_key: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    use frodo_kem_rs::{Algorithm, EncryptionKey};
    use rand_core::OsRng;

    let alg = Algorithm::FrodoKem1344Shake;
    let ek = EncryptionKey::from_bytes(alg, their_public_key)
        .ok()?;
    let (ct, ss) = alg
        .try_encapsulate_with_rng(&ek, OsRng)
        .ok()?;

    Some((ct.value().to_vec(), ss.value().to_vec()))
}

/// Decapsulate FrodoKEM-1344-SHAKE Returns shared_secret
pub fn frodo1344_decapsulate(our_secret_key: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    use frodo_kem_rs::{Algorithm, Ciphertext, DecryptionKey};

    let alg = Algorithm::FrodoKem1344Shake;
    let dk =
        DecryptionKey::from_bytes(alg, our_secret_key).ok()?;
    let ct =
        Ciphertext::from_bytes(alg, ciphertext).ok()?;
    let (ss, _msg) = alg
        .decapsulate(&dk, &ct)
        .ok()?;

    Some(ss.value().to_vec())
}

/// Generate ML-KEM-1024 keypair Returns (secret_key, public_key). FIPS 203 — the standardised module-LWE KEM, and the one every other implementation in the world will interoperate with.
pub fn generate_mlkem1024_keypair() -> (Vec<u8>, Vec<u8>) {
    use pqcrypto_mlkem::mlkem1024;
    use pqcrypto_traits::kem::{PublicKey, SecretKey};

    let (pk, sk) = mlkem1024::keypair();

    (
        SecretKey::as_bytes(&sk).to_vec(),
        PublicKey::as_bytes(&pk).to_vec(),
    )
}

/// Encapsulate ML-KEM-1024 Returns (ciphertext, 32B shared_secret)
pub fn mlkem1024_encapsulate(their_public_key: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    use pqcrypto_mlkem::mlkem1024;
    use pqcrypto_traits::kem::{Ciphertext, PublicKey, SharedSecret};

    let pk = <mlkem1024::PublicKey as PublicKey>::from_bytes(their_public_key)
        .ok()?;
    let (ss, ct) = mlkem1024::encapsulate(&pk);

    Some((
        Ciphertext::as_bytes(&ct).to_vec(),
        SharedSecret::as_bytes(&ss).to_vec(),
    ))
}

/// Decapsulate ML-KEM-1024 Returns 32B shared_secret
pub fn mlkem1024_decapsulate(our_secret_key: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    use pqcrypto_mlkem::mlkem1024;
    use pqcrypto_traits::kem::{Ciphertext, SecretKey, SharedSecret};

    let sk = <mlkem1024::SecretKey as SecretKey>::from_bytes(our_secret_key)
        .ok()?;
    let ct = <mlkem1024::Ciphertext as Ciphertext>::from_bytes(ciphertext)
        .ok()?;
    let ss = mlkem1024::decapsulate(&ct, &sk);

    Some(SharedSecret::as_bytes(&ss).to_vec())
}

/// Generate Streamlined NTRU Prime 761 keypair Returns (secret_key, public_key). A deliberately DIFFERENT lattice: NTRU Prime's field has no subfields and no ring homomorphisms, so the structural shortcuts people worry about in NTRU/ML-KEM do not apply to it.
pub fn generate_sntrup761_keypair() -> (Vec<u8>, Vec<u8>) {
    use pqcrypto_ntruprime::ntrulpr761;
    use pqcrypto_traits::kem::{PublicKey, SecretKey};

    let (pk, sk) = ntrulpr761::keypair();

    (
        SecretKey::as_bytes(&sk).to_vec(),
        PublicKey::as_bytes(&pk).to_vec(),
    )
}

/// Encapsulate NTRU Prime 761 Returns (ciphertext, 32B shared_secret)
pub fn sntrup761_encapsulate(their_public_key: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    use pqcrypto_ntruprime::ntrulpr761;
    use pqcrypto_traits::kem::{Ciphertext, PublicKey, SharedSecret};

    let pk = <ntrulpr761::PublicKey as PublicKey>::from_bytes(their_public_key)
        .ok()?;
    let (ss, ct) = ntrulpr761::encapsulate(&pk);

    Some((
        Ciphertext::as_bytes(&ct).to_vec(),
        SharedSecret::as_bytes(&ss).to_vec(),
    ))
}

/// Decapsulate NTRU Prime 761 Returns 32B shared_secret
pub fn sntrup761_decapsulate(our_secret_key: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    use pqcrypto_ntruprime::ntrulpr761;
    use pqcrypto_traits::kem::{Ciphertext, SecretKey, SharedSecret};

    let sk = <ntrulpr761::SecretKey as SecretKey>::from_bytes(our_secret_key)
        .ok()?;
    let ct = <ntrulpr761::Ciphertext as Ciphertext>::from_bytes(ciphertext)
        .ok()?;
    let ss = ntrulpr761::decapsulate(&ct, &sk);

    Some(SharedSecret::as_bytes(&ss).to_vec())
}

// ============================================================================ CLASS 2: POST-QUANTUM CODE-BASED KEMS ============================================================================

/// Generate Classic McEliece 460896 keypair Returns (secret_key, public_key ~512KB)
pub fn generate_mceliece460896_keypair() -> (Vec<u8>, Vec<u8>) {
    use classic_mceliece_rust::keypair_boxed;

    // McEliece uses a different RNG - use rng for diversity
    let mut rng = rand::thread_rng();
    let (pk, sk) = keypair_boxed(&mut rng);

    (sk.as_array().to_vec(), pk.as_array().to_vec())
}

/// Encapsulate Classic McEliece 460896 Returns (ciphertext, 32B shared_secret)
pub fn mceliece460896_encapsulate(their_public_key: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    use classic_mceliece_rust::{encapsulate_boxed, PublicKey, CRYPTO_PUBLICKEYBYTES};

    // Copy to Box for PublicKey::from
    let mut pk_box = vec![0u8; CRYPTO_PUBLICKEYBYTES].into_boxed_slice();
    pk_box.copy_from_slice(their_public_key);
    let pk_array: Box<[u8; CRYPTO_PUBLICKEYBYTES]> =
        pk_box.try_into().ok()?;
    let pk = PublicKey::from(pk_array);

    // Use rng for McEliece encapsulation (another diverse RNG source)
    let mut rng = rand::thread_rng();
    let (ct, ss) = encapsulate_boxed(&pk, &mut rng);

    Some((ct.as_array().to_vec(), ss.as_array().to_vec()))
}

/// Decapsulate Classic McEliece 460896 Returns 32B shared_secret
pub fn mceliece460896_decapsulate(our_secret_key: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    use classic_mceliece_rust::{
        decapsulate_boxed, Ciphertext, SecretKey, CRYPTO_CIPHERTEXTBYTES, CRYPTO_SECRETKEYBYTES,
    };

    // Copy to Box for SecretKey::from
    let mut sk_box = vec![0u8; CRYPTO_SECRETKEYBYTES].into_boxed_slice();
    sk_box.copy_from_slice(our_secret_key);
    let sk_array: Box<[u8; CRYPTO_SECRETKEYBYTES]> =
        sk_box.try_into().ok()?;
    let sk = SecretKey::from(sk_array);

    // Copy to array for Ciphertext::from
    let ct_array: [u8; CRYPTO_CIPHERTEXTBYTES] = ciphertext
        .try_into()
        .ok()?;
    let ct = Ciphertext::from(ct_array);

    let ss = decapsulate_boxed(&ct, &sk);

    Some(ss.as_array().to_vec())
}

/// Generate HQC-256 keypair Returns (secret_key, public_key)
pub fn generate_hqc256_keypair() -> (Vec<u8>, Vec<u8>) {
    use pqcrypto_hqc::hqc256;
    use pqcrypto_traits::kem::{PublicKey, SecretKey};

    // HQC uses PQClean's internal RNG (different from NTRU's implementation)
    let (pk, sk) = hqc256::keypair();

    (
        SecretKey::as_bytes(&sk).to_vec(),
        PublicKey::as_bytes(&pk).to_vec(),
    )
}

/// Encapsulate HQC-256 Returns (ciphertext, 64B shared_secret)
pub fn hqc256_encapsulate(their_public_key: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    use pqcrypto_hqc::hqc256;
    use pqcrypto_traits::kem::{Ciphertext, PublicKey, SharedSecret};

    let pk = <hqc256::PublicKey as PublicKey>::from_bytes(their_public_key)
        .ok()?;

    // HQC uses its own internal RNG for encapsulation
    let (ss, ct) = hqc256::encapsulate(&pk);

    Some((
        Ciphertext::as_bytes(&ct).to_vec(),
        SharedSecret::as_bytes(&ss).to_vec(),
    ))
}

/// Decapsulate HQC-256 Returns 64B shared_secret
pub fn hqc256_decapsulate(our_secret_key: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    use pqcrypto_hqc::hqc256;
    use pqcrypto_traits::kem::{Ciphertext, SecretKey, SharedSecret};

    let sk = <hqc256::SecretKey as SecretKey>::from_bytes(our_secret_key)
        .ok()?;
    let ct =
        <hqc256::Ciphertext as Ciphertext>::from_bytes(ciphertext).ok()?;

    let ss = hqc256::decapsulate(&ct, &sk);

    Some(SharedSecret::as_bytes(&ss).to_vec())
}

/// Sort two 32-byte arrays canonically (lower first)
fn sort_pair<'a>(a: &'a [u8; 32], b: &'a [u8; 32]) -> (&'a [u8; 32], &'a [u8; 32]) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

// ============================================================================ LAYER 1: CONVERSATION PROVENANCE (permanent identity binding) ============================================================================

/// Derive the conversation provenance hash.
///
/// This is a PERMANENT identifier for a conversation between two parties. It depends ONLY on identity (device keys + handle hashes + signatures), NOT on ephemeral clutch keys. This means:
/// - Same provenance survives re-clutch (key rotation)
/// - Can be used as filename/filter key for conversation messages
/// - Proves chain of custody back to initial handshake
///
/// Both parties derive the same provenance because:
/// - Device pubkeys are sorted canonically
/// - Handle hashes are sorted canonically
/// - Both signatures are included (order doesn't matter for hash)
///
/// The provenance binds:
/// - WHO: Both device pubkeys (cryptographic identity)
/// - WHAT: Both handle hashes (human-readable identity, private)
/// - PROOF: Both signatures over the handshake (proves mutual consent)
pub fn derive_conversation_provenance(
    our_device_pubkey: &[u8; 32],
    their_device_pubkey: &[u8; 32],
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    our_handshake_signature: &[u8; 64],
    their_handshake_signature: &[u8; 64],
) -> [u8; 32] {
    // Sort device pubkeys canonically
    let (first_device, second_device) = sort_pair(our_device_pubkey, their_device_pubkey);

    // Sort handle hashes canonically
    let (first_handle, second_handle) = sort_pair(our_handle_hash, their_handle_hash);

    // Signatures are included but order doesn't matter for the hash We sort by the device pubkey order for consistency
    let (first_sig, second_sig) = if our_device_pubkey < their_device_pubkey {
        (our_handshake_signature, their_handshake_signature)
    } else {
        (their_handshake_signature, our_handshake_signature)
    };

    let mut hasher = Hasher::new();
    hasher.update(b"PHOTON_PROVENANCE_v1");
    hasher.update(first_device);
    hasher.update(second_device);
    hasher.update(first_handle);
    hasher.update(second_handle);
    hasher.update(first_sig);
    hasher.update(second_sig);

    *hasher.finalize().as_bytes()
}

/// Domain separator for the time-based clutch offer provenance.
const CLUTCH_OFFER_PROV_DOMAIN: &[u8] = b"PHOTON_CLUTCH_OFFER_PROV_v2";

/// Each party's offer provenance = BLAKE3(domain, sender device pubkey, send-time oscillations).
/// This is the ORIGINAL design (see CeremonyId::derive doc: "BLAKE3(sender_pubkey || timestamp)"); an earlier drift derived it from the offer PUBKEYS, which changed on every re-key.
/// A key-based provenance made the clutch rotate: any re-key minted new pubkeys, new provenance, new ceremony_id — so over the relay both sides re-keyed on a transient egg mismatch and chased each other's moving IDs forever.
/// Time-based provenance is STABLE: the clutch does not rotate, so a party pins its send-time once (Contact::clutch_round_started), and every re-send of the same offer carries the identical time and provenance.
/// Two parties send at distinct times, so the two provenances differ and CeremonyId::derive sorts them — both sides derive the identical ceremony_id from the same sorted pair regardless of who is who.
pub fn clutch_offer_provenance(device_pubkey: &[u8; 32], send_time_osc: i64) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(CLUTCH_OFFER_PROV_DOMAIN);
    hasher.update(device_pubkey);
    hasher.update(&send_time_osc.to_le_bytes());
    *hasher.finalize().as_bytes()
}

/// Compute the handshake message that both parties sign.
///
/// This is signed by each party with their device private key. The signatures become part of the provenance derivation.
///
/// Contains: sorted device pubkeys + sorted handle hashes + timestamp The timestamp prevents replay but is NOT part of provenance (so same parties can re-establish with same provenance).
pub fn compute_handshake_message(
    our_device_pubkey: &[u8; 32],
    their_device_pubkey: &[u8; 32],
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
) -> [u8; 32] {
    let (first_device, second_device) = sort_pair(our_device_pubkey, their_device_pubkey);
    let (first_handle, second_handle) = sort_pair(our_handle_hash, their_handle_hash);

    let mut hasher = Hasher::new();
    hasher.update(b"PHOTON_HANDSHAKE v0");
    hasher.update(first_device);
    hasher.update(second_device);
    hasher.update(first_handle);
    hasher.update(second_handle);

    *hasher.finalize().as_bytes()
}

// ============================================================================ LAYER 2: clutch SEED (ephemeral encryption key material) ============================================================================

/// Derive the clutch shared seed from private handle hashes and X25519 shared secret.
///
/// This is the Phase 1 (X25519-only) seed derivation. The seed is deterministic: both parties compute the same value.
///
/// SECURITY: Uses private handle_hash = BLAKE3(handle), NOT public handle_proof!
/// - handle_proof is PUBLIC (announced to FGTW, visible in peer table)
/// - handle_hash is PRIVATE (only known to parties who know the plaintext handle)
///
/// Handle hashes are sorted canonically so order of parties doesn't matter.
///
/// Note: Phase 1 uses 32-byte seed (sufficient for single primitive). Full clutch (8 primitives) will use 256-byte seed via BLAKE3 XOF.
pub fn derive_clutch_seed_x25519(
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    x25519_shared: &[u8; 32],
) -> Seed {
    // Sort handle hashes canonically (lower first)
    let (first, second) = if our_handle_hash < their_handle_hash {
        (our_handle_hash, their_handle_hash)
    } else {
        (their_handle_hash, our_handle_hash)
    };

    let mut hasher = Hasher::new();
    hasher.update(b"clutch_x25519_only_v\x02");
    hasher.update(first);
    hasher.update(second);
    hasher.update(x25519_shared);

    Seed::from_bytes(*hasher.finalize().as_bytes())
}

/// Derive the clutch shared seed using parallel key exchange.
///
/// Both parties generate and exchange ephemeral keys simultaneously. BOTH ephemeral pubkeys contribute entropy to the final seed.
///
/// SECURITY: Uses private handle_hash = BLAKE3(handle), NOT public handle_proof!
///
/// Components are sorted canonically so order of parties doesn't matter:
/// - Device pubkeys: sorted (lower first) - binds to device identity
/// - Handle hashes: sorted (lower first)
/// - Ephemeral pubkeys: sorted (lower first)
///
/// Uses BLAKE3 XOF to produce 256-byte seed (ready for full 8-primitive clutch). Phase 1 only uses first 32 bytes, but we derive the full seed for forward compat.
pub fn derive_clutch_seed_parallel(
    our_device_pubkey: &[u8; 32],
    their_device_pubkey: &[u8; 32],
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    our_ephemeral_pub: &[u8; 32],
    their_ephemeral_pub: &[u8; 32],
    x25519_shared: &[u8; 32],
) -> Seed {
    // Sort device pubkeys canonically (binds seed to both device identities!)
    let (first_device, second_device) = sort_pair(our_device_pubkey, their_device_pubkey);

    // Sort handle hashes canonically
    let (first_handle, second_handle) = sort_pair(our_handle_hash, their_handle_hash);

    // Sort ephemeral pubkeys canonically (both contribute entropy!)
    let (first_pub, second_pub) = sort_pair(our_ephemeral_pub, their_ephemeral_pub);

    let mut hasher = Hasher::new();
    hasher.update(b"clutch v3 device bound"); // New version - device keys now bound
    hasher.update(first_device); // Device identity binding (prevents spoofing)
    hasher.update(second_device);
    hasher.update(first_handle); // Out-of-band secret (handle hash)
    hasher.update(second_handle);
    hasher.update(first_pub); // Both parties' ephemeral randomness
    hasher.update(second_pub);
    hasher.update(x25519_shared); // ECDH result (32B for X25519-only)
                                  // Future: add other 7 shared secrets here for full clutch

    // BLAKE3 XOF: extend output to 256 bytes (2048 bits) Phase 1 uses Seed (32 bytes) but we derive full output for future compat
    let mut output = [0u8; 256];
    hasher.finalize_xof().fill(&mut output);

    // For now, use first 32 bytes as seed
    let mut seed_bytes = [0u8; 32];
    seed_bytes.copy_from_slice(&output[..32]);
    Seed::from_bytes(seed_bytes)
}

/// All 8 ephemeral keypairs for full CLUTCH ceremony. Each algorithm has its own keypair format.
#[derive(Clone, Debug)]
pub struct ClutchAllKeypairs {
    // Class 0: Classical EC (32B secrets, variable pubkeys)
    pub x25519_secret: [u8; 32],
    pub x25519_public: [u8; 32],
    pub p384_secret: Vec<u8>,      // 48B
    pub p384_public: Vec<u8>,      // 97B (uncompressed SEC1)
    pub secp256k1_secret: Vec<u8>, // 32B
    pub secp256k1_public: Vec<u8>, // 65B (uncompressed SEC1)
    pub p256_secret: Vec<u8>,      // 32B
    pub p256_public: Vec<u8>,      // 65B (uncompressed SEC1)
    pub p521_secret: Vec<u8>,      // 66B
    pub p521_public: Vec<u8>,      // 133B (uncompressed SEC1)

    // Class 1: Post-quantum lattice KEMs
    pub frodo976_secret: Vec<u8>, // 31296B
    pub frodo976_public: Vec<u8>, // 15632B
    pub frodo1344_secret: Vec<u8>, // 43088B
    pub frodo1344_public: Vec<u8>, // 21520B
    pub ntru701_secret: Vec<u8>,  // 1450B (HRSS-701)
    pub ntru701_public: Vec<u8>,  // 1138B
    pub sntrup761_secret: Vec<u8>, // 1294B (NTRU Prime)
    pub sntrup761_public: Vec<u8>, // 1039B
    pub mlkem1024_secret: Vec<u8>, // 3168B
    pub mlkem1024_public: Vec<u8>, // 1568B

    // Class 2: Post-quantum code-based KEMs
    pub mceliece_secret: Vec<u8>, // 13608B
    pub mceliece_public: Vec<u8>, // 524160B (~512KB)
    pub hqc256_secret: Vec<u8>,   // 7317B
    pub hqc256_public: Vec<u8>,   // 7285B
}

impl ClutchAllKeypairs {
    /// Securely zeroize all secret keys
    pub fn zeroize(&mut self) {
        self.x25519_secret.zeroize();
        self.p384_secret.zeroize();
        self.secp256k1_secret.zeroize();
        self.p256_secret.zeroize();
        self.p521_secret.zeroize();
        self.frodo1344_secret.zeroize();
        self.sntrup761_secret.zeroize();
        self.mlkem1024_secret.zeroize();
        self.frodo976_secret.zeroize();
        self.ntru701_secret.zeroize();
        self.mceliece_secret.zeroize();
        self.hqc256_secret.zeroize();
    }
}

// ============================================================================= CLUTCH PAYLOAD STRUCTS FOR NETWORK TRANSFER =============================================================================

/// Full offer with all 8 public keys (~548KB). Sent by both parties at start of CLUTCH ceremony.
///
/// For network serialization, use the VSF-wrapped functions in protocol.rs:
/// - build_clutch_offer_vsf() / parse_clutch_offer_vsf()
#[derive(Clone, Debug, Default)]
pub struct ClutchOfferPayload {
    pub x25519_public: [u8; 32],
    pub p384_public: Vec<u8>,
    pub secp256k1_public: Vec<u8>,
    pub p256_public: Vec<u8>,
    pub p521_public: Vec<u8>,
    pub frodo976_public: Vec<u8>,
    pub frodo1344_public: Vec<u8>,
    pub ntru701_public: Vec<u8>,
    pub sntrup761_public: Vec<u8>,
    pub mlkem1024_public: Vec<u8>,
    pub mceliece_public: Vec<u8>,
    pub hqc256_public: Vec<u8>,
}

impl ClutchOfferPayload {
    /// Create from our keypairs (extract public keys)
    pub fn from_keypairs(keys: &ClutchAllKeypairs) -> Self {
        #[cfg(feature = "development")]
        crate::logf!(
            "CLUTCH: Building offer with HQC pub[..8]={}",
            hex::encode(&keys.hqc256_public[..8])
        );

        Self {
            x25519_public: keys.x25519_public,
            p384_public: keys.p384_public.clone(),
            secp256k1_public: keys.secp256k1_public.clone(),
            p256_public: keys.p256_public.clone(),
            p521_public: keys.p521_public.clone(),
            frodo976_public: keys.frodo976_public.clone(),
            frodo1344_public: keys.frodo1344_public.clone(),
            ntru701_public: keys.ntru701_public.clone(),
            sntrup761_public: keys.sntrup761_public.clone(),
            mlkem1024_public: keys.mlkem1024_public.clone(),
            mceliece_public: keys.mceliece_public.clone(), // ~512KB - PT transfer handles this
            hqc256_public: keys.hqc256_public.clone(),
        }
    }

    /// Serialize all 12 public keys to bytes (for ceremony instance derivation). Order is the wire order and must never change without a version bump — the ceremony instance hashes exactly these bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(
            32 + self.p384_public.len()
                + self.secp256k1_public.len()
                + self.p256_public.len()
                + self.p521_public.len()
                + self.frodo976_public.len()
                + self.frodo1344_public.len()
                + self.ntru701_public.len()
                + self.sntrup761_public.len()
                + self.mlkem1024_public.len()
                + self.mceliece_public.len()
                + self.hqc256_public.len(),
        );
        bytes.extend_from_slice(&self.x25519_public);
        bytes.extend_from_slice(&self.p384_public);
        bytes.extend_from_slice(&self.secp256k1_public);
        bytes.extend_from_slice(&self.p256_public);
        bytes.extend_from_slice(&self.p521_public);
        bytes.extend_from_slice(&self.frodo976_public);
        bytes.extend_from_slice(&self.frodo1344_public);
        bytes.extend_from_slice(&self.ntru701_public);
        bytes.extend_from_slice(&self.sntrup761_public);
        bytes.extend_from_slice(&self.mlkem1024_public);
        bytes.extend_from_slice(&self.mceliece_public);
        bytes.extend_from_slice(&self.hqc256_public);
        bytes
    }
}

/// KEM response with 4 PQC ciphertexts + 4 EC ephemeral pubkeys (~31KB). Sent by both parties after receiving peer's full offer.
///
/// The EC ephemeral pubkeys enable ECIES-style encapsulation: sender generates fresh keypair, computes ECDH with recipient's long-term pubkey, sends ephemeral pubkey. This gives truly distinct shared secrets per direction per algorithm.
///
/// For network serialization, use the VSF-wrapped functions in protocol.rs:
/// - build_clutch_kem_response_vsf() / parse_clutch_kem_response_vsf()
#[derive(Clone, Debug)]
pub struct ClutchKemResponsePayload {
    // PQC KEM ciphertexts (encapsulated to peer's pubkeys)
    pub frodo976_ciphertext: Vec<u8>,
    pub frodo1344_ciphertext: Vec<u8>,
    pub ntru701_ciphertext: Vec<u8>,
    pub sntrup761_ciphertext: Vec<u8>,
    pub mlkem1024_ciphertext: Vec<u8>,
    pub mceliece_ciphertext: Vec<u8>,
    pub hqc256_ciphertext: Vec<u8>,
    /// First 8 bytes of HQC public key this was encrypted to (for stale detection)
    pub target_hqc_pub_prefix: [u8; 8],
    // EC ephemeral pubkeys for ECIES-style encapsulation Sender generates fresh keypair, computes ECDH(ephemeral_secret, peer_offer_pubkey) Receiver computes ECDH(offer_secret, ephemeral_pubkey) to get same shared secret
    pub x25519_ephemeral: [u8; 32],
    pub p384_ephemeral: Vec<u8>,      // 97B uncompressed SEC1
    pub secp256k1_ephemeral: Vec<u8>, // 65B uncompressed SEC1
    pub p256_ephemeral: Vec<u8>,      // 65B uncompressed SEC1
    pub p521_ephemeral: Vec<u8>,      // 133B uncompressed SEC1
}

impl ClutchKemResponsePayload {
    /// Perform encapsulations to peer's public keys (4 PQC KEMs + 4 EC ECIES-style). Returns (payload, shared_secrets) where shared_secrets are our encapsulated secrets.
    ///
    /// For EC algorithms, we generate fresh ephemeral keypairs and compute ECDH with the peer's offer pubkeys. This gives truly distinct secrets per direction.
    /// `None` = malformed key material in the peer's offer — the caller DROPS the offer, it must never panic: an old-build or hostile offer with wrong-length keys was crashing the whole app (peer_c on v0.51.87, 2026-08-02).
    pub fn encapsulate_to_peer(their_offer: &ClutchOfferPayload) -> Option<(Self, ClutchKemSharedSecrets)> {
        #[cfg(feature = "development")]
        #[cfg(feature = "development")]
        #[cfg(feature = "development")]
        crate::log("CLUTCH: Encapsulating to peer's public keys (12 algorithms)...");

        // ===== PQC KEMs =====
        let (frodo976_ciphertext, frodo_ss) = frodo976_encapsulate(&their_offer.frodo976_public)?;
        let (frodo1344_ciphertext, frodo1344_ss) =
            frodo1344_encapsulate(&their_offer.frodo1344_public)?;
        let (ntru701_ciphertext, ntru_ss) = ntru701_encapsulate(&their_offer.ntru701_public)?;
        let (sntrup761_ciphertext, sntrup_ss) =
            sntrup761_encapsulate(&their_offer.sntrup761_public)?;
        let (mlkem1024_ciphertext, mlkem_ss) =
            mlkem1024_encapsulate(&their_offer.mlkem1024_public)?;
        let (mceliece_ciphertext, mceliece_ss) =
            mceliece460896_encapsulate(&their_offer.mceliece_public)?;
        let (hqc256_ciphertext, hqc_ss) = hqc256_encapsulate(&their_offer.hqc256_public)?;

        #[cfg(feature = "development")]
        crate::logf!(
            "CLUTCH: HQC encap: their_pub[..8]={} → ct[..8]={}",
            hex::encode(&their_offer.hqc256_public[..8]),
            hex::encode(&hqc256_ciphertext[..8])
        );

        // ===== EC ECIES-style: generate ephemeral keypairs, ECDH with peer's offer pubkeys ===== This gives distinct shared secrets per direction (we→them vs them→us)
        let (x25519_eph_secret, x25519_ephemeral) = generate_x25519_ephemeral();
        let x25519_ss = x25519_ecdh(&x25519_eph_secret, &their_offer.x25519_public);

        let (p384_eph_secret, p384_ephemeral) = generate_p384_ephemeral();
        let p384_ss = p384_ecdh(&p384_eph_secret, &their_offer.p384_public)?;

        let (secp256k1_eph_secret, secp256k1_ephemeral) = generate_secp256k1_ephemeral();
        let secp256k1_ss = secp256k1_ecdh(&secp256k1_eph_secret, &their_offer.secp256k1_public)?;

        let (p256_eph_secret, p256_ephemeral) = generate_p256_ephemeral();
        let p256_ss = p256_ecdh(&p256_eph_secret, &their_offer.p256_public)?;

        let (p521_eph_secret, p521_ephemeral) = generate_p521_ephemeral();
        let p521_ss = p521_ecdh(&p521_eph_secret, &their_offer.p521_public)?;

        #[cfg(feature = "development")]
        crate::logf!("CLUTCH: Encap ready (PQC: Frodo {}B, NTRU {}B, McEliece {}B, HQC {}B) (EC: X25519 32B, P384 {}B, secp256k1 {}B, P256 {}B)", frodo976_ciphertext.len(), ntru701_ciphertext.len(), mceliece_ciphertext.len(), hqc256_ciphertext.len(), p384_ss.len(), secp256k1_ss.len(), p256_ss.len());

        // Store the target HQC pub prefix so recipient can verify before decapsulating
        let mut target_hqc_pub_prefix = [0u8; 8];
        target_hqc_pub_prefix.copy_from_slice(&their_offer.hqc256_public[..8]);

        let payload = Self {
            frodo976_ciphertext,
            frodo1344_ciphertext,
            ntru701_ciphertext,
            sntrup761_ciphertext,
            mlkem1024_ciphertext,
            mceliece_ciphertext,
            hqc256_ciphertext,
            target_hqc_pub_prefix,
            x25519_ephemeral,
            p384_ephemeral,
            secp256k1_ephemeral,
            p256_ephemeral,
            p521_ephemeral,
        };

        let secrets = ClutchKemSharedSecrets {
            frodo: frodo_ss,
            frodo1344: frodo1344_ss,
            ntru: ntru_ss,
            sntrup: sntrup_ss,
            mlkem: mlkem_ss,
            mceliece: mceliece_ss,
            hqc: hqc_ss,
            x25519: x25519_ss,
            p384: p384_ss,
            secp256k1: secp256k1_ss,
            p256: p256_ss,
            p521: p521_ss,
        };

        Some((payload, secrets))
    }
}

/// Shared secrets from encapsulation (one direction) - all 12 algorithms. PQC KEMs produce variable-size secrets, EC ECDH produces curve-sized secrets.
#[derive(Clone, Debug)]
pub struct ClutchKemSharedSecrets {
    // PQC KEM shared secrets
    pub frodo: Vec<u8>,
    pub frodo1344: Vec<u8>,
    pub ntru: Vec<u8>,
    pub sntrup: Vec<u8>,
    pub mlkem: Vec<u8>,
    pub mceliece: Vec<u8>,
    pub hqc: Vec<u8>,
    // EC ECDH shared secrets (ECIES-style: ephemeral_secret × peer_offer_pubkey)
    pub x25519: [u8; 32],
    pub p384: Vec<u8>,      // 48B
    pub secp256k1: Vec<u8>, // 32B
    pub p256: Vec<u8>,      // 32B
    pub p521: Vec<u8>,      // 66B
}

impl ClutchKemSharedSecrets {
    /// Decapsulate from received response using our secret keys (4 PQC + 4 EC).
    ///
    /// For EC algorithms, we compute ECDH(our_offer_secret, their_ephemeral_pubkey) which gives the same shared secret as their ECDH(ephemeral_secret, our_offer_pubkey).
    pub fn decapsulate_from_peer(
        response: &ClutchKemResponsePayload,
        our_keys: &ClutchAllKeypairs,
    ) -> Option<Self> {
        #[cfg(feature = "development")]
        #[cfg(feature = "development")]
        #[cfg(feature = "development")]
        crate::log("CLUTCH: Decapsulating from peer's response (12 algorithms)...");

        // ===== PQC KEMs =====
        let frodo = frodo976_decapsulate(&our_keys.frodo976_secret, &response.frodo976_ciphertext)?;
        #[cfg(feature = "development")]
        crate::logf!(
            "CLUTCH: ✓ Frodo976 decap OK ({}B shared secret)",
            frodo.len()
        );

        let frodo1344 =
            frodo1344_decapsulate(&our_keys.frodo1344_secret, &response.frodo1344_ciphertext)?;
        let sntrup =
            sntrup761_decapsulate(&our_keys.sntrup761_secret, &response.sntrup761_ciphertext)?;
        let mlkem =
            mlkem1024_decapsulate(&our_keys.mlkem1024_secret, &response.mlkem1024_ciphertext)?;
        let ntru = ntru701_decapsulate(&our_keys.ntru701_secret, &response.ntru701_ciphertext)?;
        #[cfg(feature = "development")]
        crate::logf!("CLUTCH: ✓ NTRU701 decap OK ({}B shared secret)", ntru.len());

        // TODO: Re-enable McEliece once PT transfer is stable
        let mceliece = if response.mceliece_ciphertext.is_empty() {
            #[cfg(feature = "development")]
            #[cfg(feature = "development")]
            #[cfg(feature = "development")]
            crate::log("CLUTCH: - McEliece skipped (empty ciphertext)");
            vec![0u8; 32] // Placeholder shared secret
        } else {
            let ss = mceliece460896_decapsulate(
                &our_keys.mceliece_secret,
                &response.mceliece_ciphertext,
            )?;
            #[cfg(feature = "development")]
            crate::logf!("CLUTCH: ✓ McEliece decap OK ({}B shared secret)", ss.len());
            ss
        };

        #[cfg(feature = "development")]
        crate::logf!(
            "CLUTCH: HQC256 decap: our_sk[..8]={} their_ct[..8]={}",
            hex::encode(&our_keys.hqc256_secret[..8]),
            hex::encode(&response.hqc256_ciphertext[..8])
        );

        let hqc = hqc256_decapsulate(&our_keys.hqc256_secret, &response.hqc256_ciphertext)?;
        #[cfg(feature = "development")]
        crate::logf!("CLUTCH: ✓ HQC256 decap OK ({}B shared secret)", hqc.len());

        // ===== EC ECIES-style: ECDH(our_offer_secret, their_ephemeral_pubkey) ===== This matches their ECDH(ephemeral_secret, our_offer_pubkey)
        let x25519 = x25519_ecdh(&our_keys.x25519_secret, &response.x25519_ephemeral);
        #[cfg(feature = "development")]
        #[cfg(feature = "development")]
        #[cfg(feature = "development")]
        crate::log("CLUTCH: ✓ X25519 decap OK (32B shared secret)");

        let p384 = p384_ecdh(&our_keys.p384_secret, &response.p384_ephemeral)?;
        #[cfg(feature = "development")]
        crate::logf!("CLUTCH: ✓ P384 decap OK ({}B shared secret)", p384.len());

        let secp256k1 = secp256k1_ecdh(&our_keys.secp256k1_secret, &response.secp256k1_ephemeral)?;
        #[cfg(feature = "development")]
        crate::logf!(
            "CLUTCH: ✓ secp256k1 decap OK ({}B shared secret)",
            secp256k1.len()
        );

        let p256 = p256_ecdh(&our_keys.p256_secret, &response.p256_ephemeral)?;
        #[cfg(feature = "development")]
        crate::logf!("CLUTCH: ✓ P256 decap OK ({}B shared secret)", p256.len());

        let p521 = p521_ecdh(&our_keys.p521_secret, &response.p521_ephemeral)?;
        #[cfg(feature = "development")]
        crate::logf!("CLUTCH: ✓ P521 decap OK ({}B shared secret)", p521.len());

        Some(Self {
            frodo,
            frodo1344,
            ntru,
            sntrup,
            mlkem,
            mceliece,
            hqc,
            x25519,
            p384,
            secp256k1,
            p256,
            p521,
        })
    }

    /// Zeroize all secrets
    pub fn zeroize(&mut self) {
        self.frodo.zeroize();
        self.ntru.zeroize();
        self.mceliece.zeroize();
        self.hqc.zeroize();
        self.x25519.zeroize();
        self.p384.zeroize();
        self.secp256k1.zeroize();
        self.p256.zeroize();
    }
}

/// Sent by both parties after computing eggs to verify agreement.
///
/// Contains the eggs_proof hash. Both parties MUST compute the same proof since they derived identical eggs from the ceremony.
///
/// If proofs don't match, something went catastrophically wrong (MITM, bug, or corruption) and the ceremony MUST be aborted with a panic.
///
/// For network serialization, use the VSF-wrapped functions in protocol.rs:
/// - build_clutch_complete_vsf() / parse_clutch_complete_vsf()
#[derive(Clone, Debug)]
pub struct ClutchCompletePayload {
    pub eggs_proof: [u8; 32],
}

/// Generate all 12 ephemeral keypairs for the full CLUTCH ceremony — 5 classical curves, 5 lattice KEMs across three distinct structural assumptions, 2 code-based. WARNING: This generates ~570KB of public key material (McEliece alone is ~512KB). Caller MUST call zeroize() on the result when done!
pub fn generate_all_ephemeral_keypairs() -> ClutchAllKeypairs {
    // Class 0: Classical EC
    let (x25519_secret, x25519_public) = generate_x25519_ephemeral();
    let (p384_secret, p384_public) = generate_p384_ephemeral();
    let (secp256k1_secret, secp256k1_public) = generate_secp256k1_ephemeral();
    let (p256_secret, p256_public) = generate_p256_ephemeral();
    let (p521_secret, p521_public) = generate_p521_ephemeral();

    // Class 1: Post-quantum lattice KEMs — structured (NTRU, ML-KEM), prime-field (NTRU Prime), unstructured (Frodo, plain LWE).
    let (frodo976_secret, frodo976_public) = generate_frodo976_keypair();
    let (frodo1344_secret, frodo1344_public) = generate_frodo1344_keypair();
    let (ntru701_secret, ntru701_public) = generate_ntru701_keypair();
    let (sntrup761_secret, sntrup761_public) = generate_sntrup761_keypair();
    let (mlkem1024_secret, mlkem1024_public) = generate_mlkem1024_keypair();

    // Class 2: Post-quantum code-based KEMs
    let (mceliece_secret, mceliece_public) = generate_mceliece460896_keypair();
    let (hqc256_secret, hqc256_public) = generate_hqc256_keypair();

    ClutchAllKeypairs {
        x25519_secret,
        x25519_public,
        p384_secret,
        p384_public,
        secp256k1_secret,
        secp256k1_public,
        p256_secret,
        p256_public,
        p521_secret,
        p521_public,
        frodo976_secret,
        frodo976_public,
        frodo1344_secret,
        frodo1344_public,
        ntru701_secret,
        ntru701_public,
        sntrup761_secret,
        sntrup761_public,
        mlkem1024_secret,
        mlkem1024_public,
        mceliece_secret,
        mceliece_public,
        hqc256_secret,
        hqc256_public,
    }
}

/// All shared secrets from 20 cryptographic eggs. Each "egg" is a labeled BLAKE3 hash for domain separation.
pub struct ClutchEggs {
    pub eggs: Vec<[u8; 32]>,
}

impl ClutchEggs {
    pub fn new() -> Self {
        ClutchEggs { eggs: Vec::new() }
    }

    /// Add an egg with domain-separated labeling
    fn add_egg(&mut self, label: &str, shared_secret: &[u8]) {
        let mut hasher = Hasher::new();
        hasher.update(b"clutch EGG v\x05");
        hasher.update(label.as_bytes());
        hasher.update(shared_secret);
        self.eggs.push(*hasher.finalize().as_bytes());
    }

    /// Get eggs as slice of 32-byte arrays (for FriendshipChains::from_clutch)
    pub fn as_slice(&self) -> &[[u8; 32]] {
        &self.eggs
    }
}

/// Collect all 20 cryptographic eggs for bidirectional CLUTCH.
///
/// 4 identity eggs:
/// - our_device_pubkey, their_device_pubkey
/// - our_handle_hash, their_handle_hash
///
/// 16 shared secret eggs (8 algorithms × 2 directions):
/// - Both parties exchange in both directions
/// - Ordered by handle hash: low_* then high_*
/// - Class 0: x25519, p384, secp256k1, p256
/// - Class 1: frodo976, ntru701
/// - Class 2: mceliece460896, hqc256
///
/// Each egg is a BLAKE3 hash with domain separation: BLAKE3("clutch_EGG_v4_" || label || shared_secret)
///
/// Returns vector of 20 eggs ready for avalanche hashing.
pub fn collect_clutch_eggs(
    our_device_pubkey: &[u8; 32],
    their_device_pubkey: &[u8; 32],
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    friendship_secret: &[u8; 32],
    secrets: &ClutchSharedSecrets,
) -> ClutchEggs {
    let mut eggs = ClutchEggs::new();

    // Sort device pubkeys and handle hashes canonically so both parties add in same order
    let (low_device, high_device) = sort_pair(our_device_pubkey, their_device_pubkey);
    let (low_handle, high_handle) = sort_pair(our_handle_hash, their_handle_hash);

    eggs.add_egg("low_device_pubkey", low_device);
    eggs.add_egg("high_device_pubkey", high_device);
    eggs.add_egg("low_handle_hash", low_handle);
    eggs.add_egg("high_handle_hash", high_handle);
    // The SECRET identity binding (docs/identity-profile.md): party ids are now pinned PUBLIC identity pubkeys, so the out-of-band-secret role the private handle_hash used to play moves here — the static identity DH for friends ([`identity_friendship_secret`]), the shared identity seed for fleet siblings. Full-entropy where the old ingredient was only as private as handle guessability.
    eggs.add_egg("friendship_secret", friendship_secret);

    // Class 0: Classical EC — low handle's secrets first, then high.
    eggs.add_egg("low_x25519", &secrets.low_x25519);
    eggs.add_egg("high_x25519", &secrets.high_x25519);
    eggs.add_egg("low_p384", &secrets.low_p384);
    eggs.add_egg("high_p384", &secrets.high_p384);
    eggs.add_egg("low_secp256k1", &secrets.low_secp256k1);
    eggs.add_egg("high_secp256k1", &secrets.high_secp256k1);
    eggs.add_egg("low_p256", &secrets.low_p256);
    eggs.add_egg("high_p256", &secrets.high_p256);
    eggs.add_egg("low_p521", &secrets.low_p521);
    eggs.add_egg("high_p521", &secrets.high_p521);

    // Class 1: Post-quantum lattice KEMs — structured (NTRU, ML-KEM), prime-field (NTRU Prime), unstructured plain-LWE (Frodo).
    eggs.add_egg("low_frodo976", &secrets.low_frodo);
    eggs.add_egg("high_frodo976", &secrets.high_frodo);
    eggs.add_egg("low_frodo1344", &secrets.low_frodo1344);
    eggs.add_egg("high_frodo1344", &secrets.high_frodo1344);
    eggs.add_egg("low_ntru701", &secrets.low_ntru);
    eggs.add_egg("high_ntru701", &secrets.high_ntru);
    eggs.add_egg("low_sntrup761", &secrets.low_sntrup);
    eggs.add_egg("high_sntrup761", &secrets.high_sntrup);
    eggs.add_egg("low_mlkem1024", &secrets.low_mlkem);
    eggs.add_egg("high_mlkem1024", &secrets.high_mlkem);

    // Class 2: Post-quantum code-based KEMs
    eggs.add_egg("low_mceliece460896", &secrets.low_mceliece);
    eggs.add_egg("high_mceliece460896", &secrets.high_mceliece);
    eggs.add_egg("low_hqc256", &secrets.low_hqc);
    eggs.add_egg("high_hqc256", &secrets.high_hqc);

    eggs
}

/// Avalanche hash the eggs into dual 1MB pads for bidirectional conversation state.
///
/// This is a memory-hard, deterministic mixing function that:
/// 0. Flattens all 20 eggs into a single buffer (640 bytes)
/// 1. Repeatedly copies pseudo-random chunks to grow to 2MB
/// 2. Heavy mixing with diverse operations (+, -, *, ^, %, <<, >>)
/// 3. Final rotation and trim to exactly 2MB
/// 4. Split into two 1MB pads (low_pad, high_pad)
///
/// Properties:
/// - Deterministic: same eggs → same pads
/// - Memory-hard: 2MB total state
/// - Avalanche: every bit of input affects every bit of output
/// - Diverse operations: prevents algebraic attacks
///
/// The returned pads are saved locally:
/// - low_pad: rotates when lower handle proof sends/acks messages
/// - high_pad: rotates when higher handle proof sends/acks messages
///
/// Returns (low_pad, high_pad) as two 1MB Vec<u8> for conversation state.
pub fn avalanche_hash_eggs(eggs: &ClutchEggs) -> (Vec<u8>, Vec<u8>) {
    use i256::U256;

    #[cfg(feature = "development")]
    let start_time = std::time::Instant::now();

    #[cfg(feature = "development")]
    crate::logf!(
        "CLUTCH: Collecting {} eggs for avalanche ({} bytes input)...",
        eggs.eggs.len(),
        eggs.eggs.len() * 32
    );

    const MIN_SIZE: usize = 1_048_576; // 1MB ish
    const TOTAL_SIZE: usize = MIN_SIZE * 2; // 2MB
    let max_size = TOTAL_SIZE * 2; // Allow expansion up to 4MB

    // Step 0: Flatten all eggs into one buffer
    let mut omelette = Vec::with_capacity(max_size);
    for egg in &eggs.eggs {
        omelette.extend_from_slice(egg);
    }

    #[cfg(feature = "development")]
    let step0_elapsed = start_time.elapsed();

    let mut target_hasher = Hasher::new();
    target_hasher.update(b"target");
    target_hasher.update(&omelette);
    let target_hash = target_hasher.finalize();
    let target_u256 = U256::from_be_bytes(*target_hash.as_bytes());
    let target_size =
        TOTAL_SIZE + (target_u256 % U256::from(TOTAL_SIZE as u128)).as_u128() as usize;

    // Step 1: Grow to 2MB by copying pseudo-random chunks
    while omelette.len() < target_size {
        let current_len = omelette.len();

        // Hash current state → U256 for start position
        let mut start_hasher = Hasher::new();
        start_hasher.update(b"start");
        start_hasher.update(&omelette);
        let start_hash = start_hasher.finalize();
        let start_u256 = U256::from_be_bytes(*start_hash.as_bytes());
        let start_pos = (start_u256 % U256::from(current_len as u128)).as_u128() as usize;

        // Hash with domain separation → U256 for stop position
        let mut stop_hasher = Hasher::new();
        stop_hasher.update(&omelette);
        stop_hasher.update(b"stop");
        let stop_hash = stop_hasher.finalize();
        let stop_u256 = U256::from_be_bytes(*stop_hash.as_bytes());
        let stop_pos = (stop_u256 % U256::from(current_len as u128)).as_u128() as usize;

        // Swap if start > stop
        let (start, stop, append) = if start_pos > stop_pos {
            (stop_pos, start_pos, true)
        } else {
            (start_pos, stop_pos, false)
        };

        // Guard against empty chunk (start == stop) causing infinite loop
        let chunk = if start == stop {
            // Hash current state to get a non-empty chunk
            let mut chunk_hasher = Hasher::new();
            chunk_hasher.update(b"empty_chunk_fallback");
            chunk_hasher.update(&omelette);
            chunk_hasher.finalize().as_bytes().to_vec()
        } else {
            omelette[start..stop].to_vec()
        };
        if append {
            // Append to end
            omelette.extend_from_slice(&chunk);
        } else {
            // Prepend to start (faster than splice for large buffers)
            let mut temp = chunk;
            temp.append(&mut omelette);
            omelette = temp;
        }

        // Overgrow is okay, we'll trim at the end
        if omelette.len() > target_size {
            break;
        }
    }

    #[cfg(feature = "development")]
    let step1_elapsed = start_time.elapsed();

    // Step 2: Heavy mixing with diverse operations Process as variable-sized chunks (1-43 bytes, unaligned) for maximum diffusion
    const MIX_ROUNDS: usize = 8;

    for round in 0..MIX_ROUNDS {
        let len = omelette.len();

        // Hash current state to derive mixing parameters
        let mut round_hasher = Hasher::new();
        round_hasher.update(&omelette);
        round_hasher.update(&[round as u8]);
        let round_hash = round_hasher.finalize();
        let round_u256 = U256::from_be_bytes(*round_hash.as_bytes());

        // Determine chunk size for this round (1-43 bytes, unaligned)
        let chunk_size = 1 + ((round_u256 % U256::from(43_u128)).as_u128() as usize);

        // Mix chunks with diverse operations
        let num_chunks = len / chunk_size;

        for i in 0..num_chunks {
            let pos = i * chunk_size;
            if pos > len - chunk_size {
                break;
            }

            // Hash current chunk to derive indices
            let chunk = &omelette[pos..pos + chunk_size];
            let mut idx_hasher = Hasher::new();
            idx_hasher.update(chunk);
            idx_hasher.update(&[round as u8, i as u8]);
            let idx_hash = idx_hasher.finalize();
            let idx_u256 = U256::from_be_bytes(*idx_hash.as_bytes());

            // Pick two random chunks to mix with
            let idx1 =
                ((idx_u256 % U256::from(num_chunks as u128)).as_u128() as usize) * chunk_size;
            let idx2 = (((idx_u256 >> 64_u32) % U256::from(num_chunks as u128)).as_u128() as usize)
                * chunk_size;

            if idx1 + chunk_size > len || idx2 + chunk_size > len {
                continue;
            }

            // Read chunks (avoid borrow checker by cloning)
            let chunk1 = omelette[idx1..idx1 + chunk_size].to_vec();
            let chunk2 = omelette[idx2..idx2 + chunk_size].to_vec();

            // Apply diverse operation based on round (byte-wise for variable sizes)
            for j in 0..chunk_size {
                let val = omelette[pos + j];
                let v1 = chunk1[j];
                let v2 = chunk2[j];

                // Different operation per round for maximum diversity
                omelette[pos + j] = match round % 7 {
                    0 => val.wrapping_add(v1) ^ v2,             // + and ^
                    1 => val.wrapping_sub(v1).wrapping_mul(v2), // - and *
                    2 => (val ^ v1).wrapping_add(v2),           // ^ and +
                    3 => val.wrapping_mul(0xEF) ^ v1 ^ 0xBE,    // DEADBEEF nibbles
                    4 => (val << (v1 & 7)) ^ v2,                // << shift
                    5 => (val >> (v2 & 7)) ^ v1,                // >> shift
                    6 => val ^ v1 ^ v2 ^ 0xDE,                  // More DEADBEEF
                    _ => val,
                };
            }
        }
    }

    #[cfg(feature = "development")]
    let step2_elapsed = start_time.elapsed();

    // Step 3: Final rotation before trim Hash entire buffer and rotate by (hash % len) to shuffle one last time
    let final_hash = blake3::hash(&omelette);
    let final_u256 = U256::from_be_bytes(*final_hash.as_bytes());
    let rotate_amount = (final_u256 % U256::from(omelette.len() as u128)).as_u128() as usize;
    omelette.rotate_left(rotate_amount);

    // Step 5: Trim to exactly 2MB
    omelette.truncate(TOTAL_SIZE);

    // Step 6: Split into two 1MB pads (legacy, for backwards compat logging only)
    let low_pad = omelette[0..MIN_SIZE].to_vec();
    let high_pad = omelette[MIN_SIZE..TOTAL_SIZE].to_vec();

    #[cfg(feature = "development")]
    {
        let total_elapsed = start_time.elapsed();
        crate::logf!("CLUTCH: avalanche_hash 2MB: step0={:.1}ms step1={:.1}ms step2={:.1}ms step3={:.1}ms total={:.1}ms", step0_elapsed.as_secs_f64() * 1000.0, (step1_elapsed - step0_elapsed).as_secs_f64() * 1000.0, (step2_elapsed - step1_elapsed).as_secs_f64() * 1000.0, (total_elapsed - step2_elapsed).as_secs_f64() * 1000.0, total_elapsed.as_secs_f64() * 1000.0);
    }

    (low_pad, high_pad)
}

/// Expand eggs to 2MB mixed buffer for chain derivation.
///
/// Memory-hard, deterministic, preserves full entropy from all 20 eggs. Uses the same expansion and mixing logic as avalanche_hash_eggs but returns the full 2MB buffer instead of splitting into pads.
///
/// Properties:
/// - Deterministic: same eggs → same 2MB output
/// - Memory-hard: 2MB total state
/// - Avalanche: every bit of input affects every bit of output
/// - No compression: full 2MB preserves entropy for chain derivation
pub fn avalanche_expand_eggs(eggs: &ClutchEggs) -> Vec<u8> {
    use i256::U256;

    #[cfg(feature = "development")]
    let start_time = std::time::Instant::now();

    const TOTAL_SIZE: usize = 2_097_152; // 2MB
    let max_size = TOTAL_SIZE * 2; // Allow expansion up to 4MB

    // Step 0: Flatten all eggs into one buffer
    let mut omelette = Vec::with_capacity(max_size);
    for egg in &eggs.eggs {
        omelette.extend_from_slice(egg);
    }

    #[cfg(feature = "development")]
    let step0_elapsed = start_time.elapsed();

    // Determine target size (2-4MB, data-dependent)
    let mut target_hasher = Hasher::new();
    target_hasher.update(b"target");
    target_hasher.update(&omelette);
    let target_hash = target_hasher.finalize();
    let target_u256 = U256::from_be_bytes(*target_hash.as_bytes());
    let target_size =
        TOTAL_SIZE + (target_u256 % U256::from(TOTAL_SIZE as u128)).as_u128() as usize;

    // Step 1: Grow to target size by copying pseudo-random chunks
    while omelette.len() < target_size {
        let current_len = omelette.len();

        let mut start_hasher = Hasher::new();
        start_hasher.update(b"start");
        start_hasher.update(&omelette);
        let start_hash = start_hasher.finalize();
        let start_u256 = U256::from_be_bytes(*start_hash.as_bytes());
        let start_pos = (start_u256 % U256::from(current_len as u128)).as_u128() as usize;

        let mut stop_hasher = Hasher::new();
        stop_hasher.update(&omelette);
        stop_hasher.update(b"stop");
        let stop_hash = stop_hasher.finalize();
        let stop_u256 = U256::from_be_bytes(*stop_hash.as_bytes());
        let stop_pos = (stop_u256 % U256::from(current_len as u128)).as_u128() as usize;

        let (start, stop, append) = if start_pos > stop_pos {
            (stop_pos, start_pos, true)
        } else {
            (start_pos, stop_pos, false)
        };

        // Guard against empty chunk (start == stop) causing infinite loop
        let chunk = if start == stop {
            // Hash current state to get a non-empty chunk
            let mut chunk_hasher = Hasher::new();
            chunk_hasher.update(b"empty_chunk_fallback");
            chunk_hasher.update(&omelette);
            chunk_hasher.finalize().as_bytes().to_vec()
        } else {
            omelette[start..stop].to_vec()
        };
        if append {
            omelette.extend_from_slice(&chunk);
        } else {
            let mut temp = chunk;
            temp.append(&mut omelette);
            omelette = temp;
        }

        if omelette.len() > target_size {
            break;
        }
    }

    #[cfg(feature = "development")]
    let step1_elapsed = start_time.elapsed();

    // Step 2: Heavy mixing with diverse operations
    const MIX_ROUNDS: usize = 8;

    for round in 0..MIX_ROUNDS {
        let len = omelette.len();

        let mut round_hasher = Hasher::new();
        round_hasher.update(&omelette);
        round_hasher.update(&[round as u8]);
        let round_hash = round_hasher.finalize();
        let round_u256 = U256::from_be_bytes(*round_hash.as_bytes());

        let chunk_size = 1 + ((round_u256 % U256::from(43_u128)).as_u128() as usize);
        let num_chunks = len / chunk_size;

        for i in 0..num_chunks {
            let pos = i * chunk_size;
            if pos > len - chunk_size {
                break;
            }

            let chunk = &omelette[pos..pos + chunk_size];
            let mut idx_hasher = Hasher::new();
            idx_hasher.update(chunk);
            idx_hasher.update(&[round as u8, i as u8]);
            let idx_hash = idx_hasher.finalize();
            let idx_u256 = U256::from_be_bytes(*idx_hash.as_bytes());

            let idx1 =
                ((idx_u256 % U256::from(num_chunks as u128)).as_u128() as usize) * chunk_size;
            let idx2 = (((idx_u256 >> 64_u32) % U256::from(num_chunks as u128)).as_u128() as usize)
                * chunk_size;

            if idx1 + chunk_size > len || idx2 + chunk_size > len {
                continue;
            }

            let chunk1 = omelette[idx1..idx1 + chunk_size].to_vec();
            let chunk2 = omelette[idx2..idx2 + chunk_size].to_vec();

            for j in 0..chunk_size {
                let val = omelette[pos + j];
                let v1 = chunk1[j];
                let v2 = chunk2[j];

                omelette[pos + j] = match round % 7 {
                    0 => val.wrapping_add(v1) ^ v2,
                    1 => val.wrapping_sub(v1).wrapping_mul(v2),
                    2 => (val ^ v1).wrapping_add(v2),
                    3 => val.wrapping_mul(0xEF) ^ v1 ^ 0xBE,
                    4 => (val << (v1 & 7)) ^ v2,
                    5 => (val >> (v2 & 7)) ^ v1,
                    6 => val ^ v1 ^ v2 ^ 0xDE,
                    _ => val,
                };
            }
        }
    }

    #[cfg(feature = "development")]
    let step2_elapsed = start_time.elapsed();

    // Step 3: Final rotation
    let final_hash = blake3::hash(&omelette);
    let final_u256 = U256::from_be_bytes(*final_hash.as_bytes());
    let rotate_amount = (final_u256 % U256::from(omelette.len() as u128)).as_u128() as usize;
    omelette.rotate_left(rotate_amount);

    // Trim to exactly 2MB
    omelette.truncate(TOTAL_SIZE);

    #[cfg(feature = "development")]
    {
        let total_elapsed = start_time.elapsed();
        crate::logf!("CLUTCH: avalanche_expand 2MB: step0={:.1}ms step1={:.1}ms step2={:.1}ms step3={:.1}ms total={:.1}ms", step0_elapsed.as_secs_f64() * 1000.0, (step1_elapsed - step0_elapsed).as_secs_f64() * 1000.0, (step2_elapsed - step1_elapsed).as_secs_f64() * 1000.0, (total_elapsed - step2_elapsed).as_secs_f64() * 1000.0, total_elapsed.as_secs_f64() * 1000.0);
    }

    omelette
}

/// Derive one participant's 8KB chain from avalanche buffer.
///
/// Uses truncate-and-append for deterministic PRNG without compression. Links accumulate at end of buffer - no separate Vec needed.
///
/// Algorithm:
/// 1. Domain separation: BLAKE3_XOF(avalanche || participant) → 2MB buffer
/// 2. For 256 rounds:
/// - link = smear_hash(buffer)  // BLAKE3 ⊕ SHA3 ⊕ SHA512
/// - Drop first 32B, append link at end
/// 3. Chain = last 8KB (256 links in order)
pub fn derive_chain_from_avalanche(avalanche: &[u8], participant: &[u8; 32]) -> Vec<u8> {
    // Domain separation: mix participant identity into state
    let mut hasher = Hasher::new();
    hasher.update(b"PHOTON_CHAIN_DERIVE_v\x02");
    hasher.update(participant);
    hasher.update(avalanche);

    // Create working buffer from domain-separated XOF
    let mut buffer = vec![0u8; avalanche.len()];
    hasher.finalize_xof().fill(&mut buffer);
    let len = buffer.len();

    // Generate 256 links via truncate-and-append Each link = smear_hash(buffer), then drop first 32B, append link at end
    for _ in 0..256 {
        let link = smear_hash(&buffer);
        buffer.copy_within(32.., 0); // shift left, drop first 32B
        buffer[len - 32..].copy_from_slice(&link); // append link at end
    }

    // Chain = last 8KB (256 × 32B links in order)
    buffer[len - 8192..].to_vec()
}

/// Compute the clutch completion proof. Sent by initiator to confirm they derived the same seed. Responder can verify without revealing the seed.
pub fn compute_clutch_proof(seed: &Seed) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(seed.as_bytes());
    hasher.update(b"clutch_complete_v\x02");
    *hasher.finalize().as_bytes()
}

/// Verify the clutch completion proof matches our derived seed.
pub fn verify_clutch_proof(seed: &Seed, proof: &[u8; 32]) -> bool {
    use subtle::ConstantTimeEq;
    let expected = compute_clutch_proof(seed);
    // Actually constant-time comparison (subtle crate)
    expected.ct_eq(proof).into()
}

/// Full clutch ceremony result
pub struct ClutchResult {
    pub seed: Seed,
    pub proof: [u8; 32],
}

/// Perform complete clutch ceremony using parallel key exchange.
///
/// Both parties generate ephemeral keypairs simultaneously and exchange them. Both pubkeys contribute entropy to the final seed. Device pubkeys are mixed in to bind the seed to both device identities.
///
/// Steps:
/// 0. Generate ephemeral keypair (done before calling this)
/// 1. Exchange ClutchOffer messages (both directions, parallel)
/// 2. Once both pubkeys known, call this function
/// 3. Lower handle_proof party sends ClutchComplete with proof
///
/// SECURITY: Takes private handle_hash = BLAKE3(handle), NOT public handle_proof! Device pubkeys are mixed in to prevent handle spoofing with different device.
pub fn clutch_complete_parallel(
    our_device_pubkey: &[u8; 32],
    their_device_pubkey: &[u8; 32],
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    our_ephemeral_secret: &[u8; 32],
    our_ephemeral_pubkey: &[u8; 32],
    their_ephemeral_pubkey: &[u8; 32],
) -> ClutchResult {
    let mut x25519_shared = x25519_ecdh(our_ephemeral_secret, their_ephemeral_pubkey);
    let seed = derive_clutch_seed_parallel(
        our_device_pubkey,
        their_device_pubkey,
        our_handle_hash,
        their_handle_hash,
        our_ephemeral_pubkey,
        their_ephemeral_pubkey,
        &x25519_shared,
    );
    let proof = compute_clutch_proof(&seed);

    // Zeroize intermediate shared secret
    x25519_shared.zeroize();

    ClutchResult { seed, proof }
}

/// Full CLUTCH result with eggs for chain derivation.
pub struct ClutchFullResult {
    /// 20 cryptographic eggs from the ceremony (for FriendshipChains)
    pub eggs: ClutchEggs,
    /// Proof hash for verification
    pub proof: [u8; 32],
}

/// All 16 shared secrets for full CLUTCH (8 algorithms × 2 directions).
///
/// All algorithms now use ECIES-style bidirectional encapsulation:
/// - Each party generates ephemeral keys and encapsulates to peer
/// - Results in TWO distinct shared secrets per algorithm (truly bidirectional)
/// - low_* = encapsulated by lower handle_hash party
/// - high_* = encapsulated by higher handle_hash party
///
/// For 2-party: 16 distinct shared secrets (8 algorithms × 2 directions) For 3-party: 48 distinct shared secrets (8 algorithms × 6 directed pairs)
///
/// An attacker must compromise BOTH directions of an algorithm to break that algorithm's contribution to the final key material.
pub struct ClutchSharedSecrets {
    // Class 0: Classical EC (ECIES-style: distinct secret per direction)
    pub low_x25519: [u8; 32],
    pub high_x25519: [u8; 32],
    pub low_p384: Vec<u8>, // 48B
    pub high_p384: Vec<u8>,
    pub low_secp256k1: Vec<u8>, // 32B
    pub high_secp256k1: Vec<u8>,
    pub low_p256: Vec<u8>, // 32B
    pub high_p256: Vec<u8>,
    pub low_p521: Vec<u8>, // 66B
    pub high_p521: Vec<u8>,

    // Class 1: Post-quantum lattice KEMs (distinct secret per direction)
    pub low_frodo: Vec<u8>, // 24B
    pub high_frodo: Vec<u8>,
    pub low_frodo1344: Vec<u8>, // 32B
    pub high_frodo1344: Vec<u8>,
    pub low_ntru: Vec<u8>, // 32B
    pub high_ntru: Vec<u8>,
    pub low_sntrup: Vec<u8>, // 32B
    pub high_sntrup: Vec<u8>,
    pub low_mlkem: Vec<u8>, // 32B
    pub high_mlkem: Vec<u8>,

    // Class 2: Post-quantum code-based KEMs (distinct secret per direction)
    pub low_mceliece: Vec<u8>, // 32B
    pub high_mceliece: Vec<u8>,
    pub low_hqc: Vec<u8>, // 64B
    pub high_hqc: Vec<u8>,
}

impl ClutchSharedSecrets {
    /// Securely zeroize all shared secrets
    pub fn zeroize(&mut self) {
        self.low_x25519.zeroize();
        self.high_x25519.zeroize();
        self.low_p384.zeroize();
        self.high_p384.zeroize();
        self.low_secp256k1.zeroize();
        self.high_secp256k1.zeroize();
        self.low_p256.zeroize();
        self.high_p256.zeroize();
        self.low_p521.zeroize();
        self.high_p521.zeroize();
        self.low_frodo.zeroize();
        self.high_frodo.zeroize();
        self.low_frodo1344.zeroize();
        self.high_frodo1344.zeroize();
        self.low_ntru.zeroize();
        self.high_ntru.zeroize();
        self.low_sntrup.zeroize();
        self.high_sntrup.zeroize();
        self.low_mlkem.zeroize();
        self.high_mlkem.zeroize();
        self.low_mceliece.zeroize();
        self.high_mceliece.zeroize();
        self.low_hqc.zeroize();
        self.high_hqc.zeroize();
    }
}

/// Perform the full 12-algorithm CLUTCH ceremony.
///
/// Takes all 24 shared secrets (12 algorithms × 2 directions) and produces identical (low_pad, high_pad) on both parties.
///
/// The low/high ordering is determined by comparing handle_hashes:
/// - Party with lower handle_hash uses low_pad for sending
/// - Party with higher handle_hash uses high_pad for sending
///
/// Both parties MUST call this with the same shared secrets (just with their perspective on low/high being different based on handle ordering).
///
/// Returns ClutchFullResult with:
/// - low_pad: 1MB encryption pad for low handle party
/// - high_pad: 1MB encryption pad for high handle party
/// - proof: BLAKE3 hash of pads for verification
pub fn clutch_complete_full(
    our_device_pubkey: &[u8; 32],
    their_device_pubkey: &[u8; 32],
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    friendship_secret: &[u8; 32],
    secrets: &ClutchSharedSecrets,
) -> ClutchFullResult {
    // Collect the eggs (24 KEM + 4 identity + the friendship-secret egg). Passing the struct rather than 24 positional slices is deliberate: the old call listed every secret by position, and a single transposed pair there would silently produce a different-but-valid pad on one side only.
    let eggs = collect_clutch_eggs(
        our_device_pubkey,
        their_device_pubkey,
        our_handle_hash,
        their_handle_hash,
        friendship_secret,
        secrets,
    );

    // Compute proof from eggs (deterministic - same eggs = same proof)
    let proof = compute_eggs_proof(&eggs);

    ClutchFullResult { eggs, proof }
}

/// Compute proof hash for CLUTCH verification from eggs. Used by both parties to verify they collected identical eggs.
///
/// Defense-in-depth: uses spaghettify + smear_hash for algorithm diversity. If BLAKE3 is broken, SHA3 and SHA512 still protect the proof. If any hash is broken, spaghettify's chaos mixing still scrambles the eggs.
pub fn compute_eggs_proof(eggs: &ClutchEggs) -> [u8; 32] {
    // Flatten eggs to bytes
    let mut egg_bytes = Vec::with_capacity(eggs.eggs.len() * 32);
    for egg in &eggs.eggs {
        egg_bytes.extend_from_slice(egg);
    }

    // Add domain separation
    let mut input = b"CLUTCH_EGGS_proof_v\x03".to_vec();
    input.extend_from_slice(&egg_bytes);

    // Spaghettify for chaos mixing, then smear_hash for algorithm diversity This is overkill for a proof, but consistency with chain derivation is good
    let spaghetti = spaghettify(&input);

    // Final proof = smear_hash(spaghetti || eggs) Defense in depth: if spaghettify broken, eggs still contribute directly
    let mut final_input = spaghetti.to_vec();
    final_input.extend_from_slice(&egg_bytes);
    smear_hash(&final_input)
}

/// Verify CLUTCH proof matches our eggs.
pub fn verify_eggs_proof(eggs: &ClutchEggs, proof: &[u8; 32]) -> bool {
    use subtle::ConstantTimeEq;
    let expected = compute_eggs_proof(eggs);
    expected.ct_eq(proof).into()
}

#[cfg(test)]
mod tests {
    /// A deterministic full secret set for tests — one place that knows all 24 slots, so a test never lists them positionally.
    fn fixture_secrets() -> ClutchSharedSecrets {
        let v = |b: u8, n: usize| vec![b; n];
        ClutchSharedSecrets {
            low_x25519: [5u8; 32],
            high_x25519: [6u8; 32],
            low_p384: v(7, 48),
            high_p384: v(8, 48),
            low_secp256k1: v(9, 32),
            high_secp256k1: v(10, 32),
            low_p256: v(19, 32),
            high_p256: v(20, 32),
            low_p521: v(21, 66),
            high_p521: v(22, 66),
            low_frodo: v(11, 24),
            high_frodo: v(12, 24),
            low_frodo1344: v(23, 32),
            high_frodo1344: v(24, 32),
            low_ntru: v(13, 32),
            high_ntru: v(14, 32),
            low_sntrup: v(25, 32),
            high_sntrup: v(26, 32),
            low_mlkem: v(27, 32),
            high_mlkem: v(28, 32),
            low_mceliece: v(15, 32),
            high_mceliece: v(16, 32),
            low_hqc: v(17, 64),
            high_hqc: v(18, 64),
        }
    }

    use super::*;

    #[test]
    fn test_initiator_selection_deterministic() {
        let alice = [1u8; 32];
        let bob = [2u8; 32];

        // Alice (lower) should be initiator from both perspectives
        assert!(is_clutch_initiator(&alice, &bob));
        assert!(!is_clutch_initiator(&bob, &alice));
    }

    #[test]
    fn test_clutch_ceremony_v1_compatibility_removed() {
        // This test verified v1 sequential clutch (initiator/responder pattern). v3 uses parallel exchange only - see test_parallel_clutch_produces_same_seed. Keeping this stub to document the intentional removal of v1 support.
    }

    #[test]
    fn sibling_party_id_deterministic_and_domain_separated() {
        let device = [7u8; 32];
        // Deterministic — both sides of a sibling pair derive the same pid for the same device.
        assert_eq!(sibling_party_id(&device), sibling_party_id(&device));
        // Distinct per device.
        assert_ne!(sibling_party_id(&device), sibling_party_id(&[8u8; 32]));
        // Domain-separated: never a bare BLAKE3 of the pubkey (so a sibling pid can't collide with any hash-of-something-public id space by construction).
        assert_ne!(sibling_party_id(&device), *blake3::hash(&device).as_bytes());
    }

    #[test]
    fn sibling_pair_tokens_symmetric_and_distinct_across_fleet() {
        // A 3-device fleet has 3 sibling pairs; every pair must derive the SAME conversation token on both sides (order-independent) and a DIFFERENT token from every other pair — this is exactly the collision the shared handle_hash caused before the party-id seam.
        let pids: Vec<[u8; 32]> = [[1u8; 32], [2u8; 32], [3u8; 32]]
            .iter()
            .map(sibling_party_id)
            .collect();
        let mut tokens = Vec::new();
        for i in 0..pids.len() {
            for j in (i + 1)..pids.len() {
                let t_ab = derive_conversation_token(&[pids[i], pids[j]]);
                let t_ba = derive_conversation_token(&[pids[j], pids[i]]);
                assert_eq!(t_ab, t_ba, "token must be order-independent");
                tokens.push(t_ab);
            }
        }
        tokens.sort_unstable();
        tokens.dedup();
        assert_eq!(
            tokens.len(),
            3,
            "each sibling pair must get a distinct token"
        );
    }

    #[test]
    fn test_different_handles_different_seeds() {
        // Private handle hashes (BLAKE3 of plaintext handle)
        let handle_hash1 = *blake3::hash(b"handle one").as_bytes();
        let handle_hash2 = *blake3::hash(b"handle two").as_bytes();
        let handle_hash3 = *blake3::hash(b"handle three").as_bytes();

        let (secret, public) = generate_x25519_ephemeral();
        let shared = x25519_ecdh(&secret, &public);

        let seed_12 = derive_clutch_seed_x25519(&handle_hash1, &handle_hash2, &shared);
        let seed_13 = derive_clutch_seed_x25519(&handle_hash1, &handle_hash3, &shared);

        // Different handle pairs should produce different seeds
        assert_ne!(seed_12.as_bytes(), seed_13.as_bytes());
    }

    #[test]
    fn test_parallel_clutch_produces_same_seed() {
        // Device pubkeys (simulated Ed25519 public keys)
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];

        // Private handle hashes
        let alice_handle_hash = *blake3::hash(b"alice parallel handle").as_bytes();
        let bob_handle_hash = *blake3::hash(b"bob parallel handle").as_bytes();

        // Both generate ephemeral keypairs simultaneously
        let (alice_secret, alice_public) = generate_x25519_ephemeral();
        let (bob_secret, bob_public) = generate_x25519_ephemeral();

        // Both complete the ceremony with device keys and all four pubkeys
        let alice_result = clutch_complete_parallel(
            &alice_device,
            &bob_device,
            &alice_handle_hash,
            &bob_handle_hash,
            &alice_secret,
            &alice_public,
            &bob_public,
        );

        let bob_result = clutch_complete_parallel(
            &bob_device,
            &alice_device,
            &bob_handle_hash,
            &alice_handle_hash,
            &bob_secret,
            &bob_public,
            &alice_public,
        );

        // Both should derive the same seed
        assert_eq!(alice_result.seed.as_bytes(), bob_result.seed.as_bytes());

        // Proofs should match
        assert_eq!(alice_result.proof, bob_result.proof);

        // Cross-verify proofs
        assert!(verify_clutch_proof(&alice_result.seed, &bob_result.proof));
        assert!(verify_clutch_proof(&bob_result.seed, &alice_result.proof));
    }

    #[test]
    fn test_parallel_sorted_pubkeys_deterministic() {
        // Verify that sorting pubkeys produces deterministic output regardless of order
        let device1 = [1u8; 32];
        let device2 = [2u8; 32];
        let handle1 = *blake3::hash(b"handle 1").as_bytes();
        let handle2 = *blake3::hash(b"handle 2").as_bytes();

        let (secret1, pub1) = generate_x25519_ephemeral();
        let (_secret2, pub2) = generate_x25519_ephemeral();

        let shared = x25519_ecdh(&secret1, &pub2);

        // Derive seed with pubkeys in both orders - should produce same result
        let seed_a = derive_clutch_seed_parallel(
            &device1, &device2, &handle1, &handle2, &pub1, &pub2, &shared,
        );
        let seed_b = derive_clutch_seed_parallel(
            &device1, &device2, &handle1, &handle2, &pub2, &pub1, &shared,
        );

        assert_eq!(seed_a.as_bytes(), seed_b.as_bytes());
    }

    #[test]
    fn test_different_device_keys_different_seeds() {
        // Different device keys should produce different seeds (prevents spoofing)
        let device1 = [1u8; 32];
        let device2 = [2u8; 32];
        let device3 = [3u8; 32]; // Attacker's device
        let handle1 = *blake3::hash(b"alice").as_bytes();
        let handle2 = *blake3::hash(b"bob").as_bytes();

        let (secret1, pub1) = generate_x25519_ephemeral();
        let (_secret2, pub2) = generate_x25519_ephemeral();

        let shared = x25519_ecdh(&secret1, &pub2);

        // Legitimate seed between device1 and device2
        let legit_seed = derive_clutch_seed_parallel(
            &device1, &device2, &handle1, &handle2, &pub1, &pub2, &shared,
        );

        // Attacker tries to spoof with device3 claiming to be bob
        let spoofed_seed = derive_clutch_seed_parallel(
            &device1, &device3, &handle1, &handle2, &pub1, &pub2, &shared,
        );

        // Seeds MUST be different - device key binding prevents spoofing
        assert_ne!(legit_seed.as_bytes(), spoofed_seed.as_bytes());
    }

    // ======================================================================== PROVENANCE TESTS ========================================================================

    /// The time-based offer provenance: each party derives its own from (its device key, its pinned send-time), and both parties feed the two provenances into CeremonyId::derive, which SORTS them — so both sides compute the identical ceremony_id regardless of who is A and who is B, and regardless of whose time is larger. This is what makes the clutch converge without rotation.
    #[test]
    fn clutch_offer_provenance_sorts_to_same_ceremony_id() {
        use crate::types::friendship::CeremonyId;
        let a_device = [0x11u8; 32];
        let b_device = [0x22u8; 32];
        let a_handle = *blake3::hash(b"party-a").as_bytes();
        let b_handle = *blake3::hash(b"party-b").as_bytes();
        // Two DISTINCT send-times (mine and yours), as in a real exchange.
        let a_time: i64 = 1_000_000_000;
        let b_time: i64 = 1_000_050_000;

        let a_prov = clutch_offer_provenance(&a_device, a_time);
        let b_prov = clutch_offer_provenance(&b_device, b_time);
        assert_ne!(
            a_prov, b_prov,
            "distinct parties/times → distinct provenances"
        );

        // A collected [its own, then B's]; B collected [its own, then A's] — opposite order. derive() sorts, so both land on the same id.
        let id_from_a = CeremonyId::derive(&[a_handle, b_handle], &[a_prov, b_prov]);
        let id_from_b = CeremonyId::derive(&[b_handle, a_handle], &[b_prov, a_prov]);
        assert_eq!(
            id_from_a.as_bytes(),
            id_from_b.as_bytes(),
            "both sides derive the SAME ceremony_id from the sorted provenance pair"
        );

        // Re-sending the SAME offer (same pinned time) yields the SAME provenance → same id → no rotation.
        let a_prov_resend = clutch_offer_provenance(&a_device, a_time);
        assert_eq!(
            a_prov, a_prov_resend,
            "a re-send with the pinned time is byte-identical — the clutch does not rotate"
        );
    }

    #[test]
    fn test_provenance_deterministic_both_parties() {
        // Both parties should derive the same provenance
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];
        let alice_handle = *blake3::hash(b"alice").as_bytes();
        let bob_handle = *blake3::hash(b"bob").as_bytes();

        // Simulated signatures (in real code, these come from Ed25519 signing)
        let alice_sig = [0xAAu8; 64];
        let bob_sig = [0xBBu8; 64];

        let alice_provenance = derive_conversation_provenance(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &alice_sig,
            &bob_sig,
        );

        let bob_provenance = derive_conversation_provenance(
            &bob_device,
            &alice_device,
            &bob_handle,
            &alice_handle,
            &bob_sig,
            &alice_sig,
        );

        // Both parties MUST derive the same provenance
        assert_eq!(alice_provenance, bob_provenance);
    }

    #[test]
    fn test_provenance_different_for_different_pairs() {
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];
        let charlie_device = [3u8; 32];
        let alice_handle = *blake3::hash(b"alice").as_bytes();
        let bob_handle = *blake3::hash(b"bob").as_bytes();
        let charlie_handle = *blake3::hash(b"charlie").as_bytes();

        let sig_a = [0xAAu8; 64];
        let sig_b = [0xBBu8; 64];
        let sig_c = [0xCCu8; 64];

        let alice_bob = derive_conversation_provenance(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &sig_a,
            &sig_b,
        );

        let alice_charlie = derive_conversation_provenance(
            &alice_device,
            &charlie_device,
            &alice_handle,
            &charlie_handle,
            &sig_a,
            &sig_c,
        );

        // Different conversation pairs MUST have different provenance
        assert_ne!(alice_bob, alice_charlie);
    }

    #[test]
    fn test_provenance_survives_reclutch() {
        // Key insight: provenance doesn't include ephemeral keys So re-clutch (new ephemeral keys) produces same provenance
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];
        let alice_handle = *blake3::hash(b"alice").as_bytes();
        let bob_handle = *blake3::hash(b"bob").as_bytes();
        let alice_sig = [0xAAu8; 64];
        let bob_sig = [0xBBu8; 64];

        // First clutch - get provenance
        let provenance_1 = derive_conversation_provenance(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &alice_sig,
            &bob_sig,
        );

        // Re-clutch with new ephemeral keys (simulated) But same device keys, handles, and signatures
        let provenance_2 = derive_conversation_provenance(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &alice_sig,
            &bob_sig,
        );

        // Provenance MUST be identical after re-clutch
        assert_eq!(provenance_1, provenance_2);
    }

    #[test]
    fn test_handshake_message_deterministic() {
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];
        let alice_handle = *blake3::hash(b"alice").as_bytes();
        let bob_handle = *blake3::hash(b"bob").as_bytes();

        let alice_msg =
            compute_handshake_message(&alice_device, &bob_device, &alice_handle, &bob_handle);

        let bob_msg =
            compute_handshake_message(&bob_device, &alice_device, &bob_handle, &alice_handle);

        // Both parties compute the same handshake message to sign
        assert_eq!(alice_msg, bob_msg);
    }

    #[test]
    fn test_egg_collection_produces_vector() {
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];
        let alice_handle = *blake3::hash(b"alice").as_bytes();
        let bob_handle = *blake3::hash(b"bob").as_bytes();

        // 16 shared secrets (8 algorithms × 2 directions)
        let eggs = collect_clutch_eggs(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &[0x5Au8; 32], // friendship secret (symmetric fixture)
            &fixture_secrets(),
        );

        // 4 identity + 1 friendship secret + 24 shared secrets (12 algorithms x 2 directions) = 29 eggs
        assert_eq!(eggs.eggs.len(), 29);

        for egg in &eggs.eggs {
            assert_eq!(egg.len(), 32);
        }
    }

    #[test]
    fn test_egg_domain_separation() {
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];
        let alice_handle = *blake3::hash(b"alice").as_bytes();
        let bob_handle = *blake3::hash(b"bob").as_bytes();

        // Use same bytes for all secrets to test domain separation
        let uniform = {
            let mut f = fixture_secrets();
            // Same value in every slot: proves the eggs are LABEL-separated, not value-separated.
            let v32 = vec![99u8; 32];
            f.low_x25519 = [99u8; 32];
            f.high_x25519 = [99u8; 32];
            f.low_p384 = vec![99u8; 48];
            f.high_p384 = vec![99u8; 48];
            f.low_secp256k1 = v32.clone();
            f.high_secp256k1 = v32.clone();
            f.low_p256 = v32.clone();
            f.high_p256 = v32.clone();
            f.low_p521 = vec![99u8; 66];
            f.high_p521 = vec![99u8; 66];
            f.low_frodo = vec![99u8; 24];
            f.high_frodo = vec![99u8; 24];
            f.low_frodo1344 = v32.clone();
            f.high_frodo1344 = v32.clone();
            f.low_ntru = v32.clone();
            f.high_ntru = v32.clone();
            f.low_sntrup = v32.clone();
            f.high_sntrup = v32.clone();
            f.low_mlkem = v32.clone();
            f.high_mlkem = v32.clone();
            f.low_mceliece = v32.clone();
            f.high_mceliece = v32;
            f.low_hqc = vec![99u8; 64];
            f.high_hqc = vec![99u8; 64];
            f
        };
        let eggs = collect_clutch_eggs(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &[0x5Au8; 32],
            &uniform,
        );

        // Even with identical input bytes in every slot, the per-egg domain labels must make all 29 distinct — that is what proves the eggs are separated by NAME and not by value.
        let unique_eggs: std::collections::HashSet<[u8; 32]> = eggs.eggs.into_iter().collect();
        assert_eq!(unique_eggs.len(), 29);
    }

    #[test]
    fn test_full_clutch_identical_pads() {
        // This is THE critical test: both parties must derive identical pads

        // Device identities
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];

        // Handle hashes (alice < bob so alice is "low")
        let alice_handle = *blake3::hash(b"alice").as_bytes();
        let bob_handle = *blake3::hash(b"bob").as_bytes();
        assert!(alice_handle < bob_handle, "alice should be low handle");

        // Generate all keypairs for both parties
        let mut alice_keys = generate_all_ephemeral_keypairs();
        let mut bob_keys = generate_all_ephemeral_keypairs();

        // === EC ALGORITHMS: Both compute same shared secret === X25519
        let x25519_shared = x25519_ecdh(&alice_keys.x25519_secret, &bob_keys.x25519_public);
        let x25519_shared_bob = x25519_ecdh(&bob_keys.x25519_secret, &alice_keys.x25519_public);
        assert_eq!(x25519_shared, x25519_shared_bob);

        // P-384
        let p384_shared = p384_ecdh(&alice_keys.p384_secret, &bob_keys.p384_public).unwrap();
        let p384_shared_bob = p384_ecdh(&bob_keys.p384_secret, &alice_keys.p384_public).unwrap();
        assert_eq!(p384_shared, p384_shared_bob);

        // secp256k1
        let secp256k1_shared =
            secp256k1_ecdh(&alice_keys.secp256k1_secret, &bob_keys.secp256k1_public).unwrap();
        let secp256k1_shared_bob =
            secp256k1_ecdh(&bob_keys.secp256k1_secret, &alice_keys.secp256k1_public).unwrap();
        assert_eq!(secp256k1_shared, secp256k1_shared_bob);

        // P-256
        let p256_shared = p256_ecdh(&alice_keys.p256_secret, &bob_keys.p256_public).unwrap();
        let p256_shared_bob = p256_ecdh(&bob_keys.p256_secret, &alice_keys.p256_public).unwrap();
        assert_eq!(p256_shared, p256_shared_bob);

        // P-521
        let p521_shared = p521_ecdh(&alice_keys.p521_secret, &bob_keys.p521_public).unwrap();
        let p521_shared_bob = p521_ecdh(&bob_keys.p521_secret, &alice_keys.p521_public).unwrap();
        assert_eq!(p521_shared, p521_shared_bob);

        // === KEM ALGORITHMS: Each encapsulates to peer, decapsulates own ===

        // FrodoKEM-976
        let (frodo_ct_to_bob, frodo_ss_alice_encap) =
            frodo976_encapsulate(&bob_keys.frodo976_public).unwrap();
        let (frodo_ct_to_alice, frodo_ss_bob_encap) =
            frodo976_encapsulate(&alice_keys.frodo976_public).unwrap();
        let frodo_ss_bob_decap = frodo976_decapsulate(&bob_keys.frodo976_secret, &frodo_ct_to_bob).unwrap();
        let frodo_ss_alice_decap =
            frodo976_decapsulate(&alice_keys.frodo976_secret, &frodo_ct_to_alice).unwrap();
        assert_eq!(frodo_ss_alice_encap, frodo_ss_bob_decap); // Alice→Bob direction
        assert_eq!(frodo_ss_bob_encap, frodo_ss_alice_decap); // Bob→Alice direction

        // NTRU-701
        let (ntru_ct_to_bob, ntru_ss_alice_encap) = ntru701_encapsulate(&bob_keys.ntru701_public).unwrap();
        let (ntru_ct_to_alice, ntru_ss_bob_encap) = ntru701_encapsulate(&alice_keys.ntru701_public).unwrap();
        let ntru_ss_bob_decap = ntru701_decapsulate(&bob_keys.ntru701_secret, &ntru_ct_to_bob).unwrap();
        let ntru_ss_alice_decap =
            ntru701_decapsulate(&alice_keys.ntru701_secret, &ntru_ct_to_alice).unwrap();
        assert_eq!(ntru_ss_alice_encap, ntru_ss_bob_decap);
        assert_eq!(ntru_ss_bob_encap, ntru_ss_alice_decap);

        // McEliece-460896
        let (mce_ct_to_bob, mce_ss_alice_encap) =
            mceliece460896_encapsulate(&bob_keys.mceliece_public).unwrap();
        let (mce_ct_to_alice, mce_ss_bob_encap) =
            mceliece460896_encapsulate(&alice_keys.mceliece_public).unwrap();
        let mce_ss_bob_decap =
            mceliece460896_decapsulate(&bob_keys.mceliece_secret, &mce_ct_to_bob).unwrap();
        let mce_ss_alice_decap =
            mceliece460896_decapsulate(&alice_keys.mceliece_secret, &mce_ct_to_alice).unwrap();
        assert_eq!(mce_ss_alice_encap, mce_ss_bob_decap);
        assert_eq!(mce_ss_bob_encap, mce_ss_alice_decap);

        // HQC-256
        let (hqc_ct_to_bob, hqc_ss_alice_encap) = hqc256_encapsulate(&bob_keys.hqc256_public).unwrap();
        let (hqc_ct_to_alice, hqc_ss_bob_encap) = hqc256_encapsulate(&alice_keys.hqc256_public).unwrap();
        let hqc_ss_bob_decap = hqc256_decapsulate(&bob_keys.hqc256_secret, &hqc_ct_to_bob).unwrap();
        let hqc_ss_alice_decap = hqc256_decapsulate(&alice_keys.hqc256_secret, &hqc_ct_to_alice).unwrap();
        assert_eq!(hqc_ss_alice_encap, hqc_ss_bob_decap);
        assert_eq!(hqc_ss_bob_encap, hqc_ss_alice_decap);

        // FrodoKEM-1344
        let (f13_ct_to_bob, f13_alice_encap) = frodo1344_encapsulate(&bob_keys.frodo1344_public).unwrap();
        let (f13_ct_to_alice, f13_bob_encap) = frodo1344_encapsulate(&alice_keys.frodo1344_public).unwrap();
        let f13_bob_decap = frodo1344_decapsulate(&bob_keys.frodo1344_secret, &f13_ct_to_bob).unwrap();
        let f13_alice_decap = frodo1344_decapsulate(&alice_keys.frodo1344_secret, &f13_ct_to_alice).unwrap();
        assert_eq!(f13_alice_encap, f13_bob_decap);
        assert_eq!(f13_bob_encap, f13_alice_decap);

        // NTRU Prime 761
        let (sn_ct_to_bob, sn_alice_encap) = sntrup761_encapsulate(&bob_keys.sntrup761_public).unwrap();
        let (sn_ct_to_alice, sn_bob_encap) = sntrup761_encapsulate(&alice_keys.sntrup761_public).unwrap();
        let sn_bob_decap = sntrup761_decapsulate(&bob_keys.sntrup761_secret, &sn_ct_to_bob).unwrap();
        let sn_alice_decap = sntrup761_decapsulate(&alice_keys.sntrup761_secret, &sn_ct_to_alice).unwrap();
        assert_eq!(sn_alice_encap, sn_bob_decap);
        assert_eq!(sn_bob_encap, sn_alice_decap);

        // ML-KEM-1024
        let (ml_ct_to_bob, ml_alice_encap) = mlkem1024_encapsulate(&bob_keys.mlkem1024_public).unwrap();
        let (ml_ct_to_alice, ml_bob_encap) = mlkem1024_encapsulate(&alice_keys.mlkem1024_public).unwrap();
        let ml_bob_decap = mlkem1024_decapsulate(&bob_keys.mlkem1024_secret, &ml_ct_to_bob).unwrap();
        let ml_alice_decap = mlkem1024_decapsulate(&alice_keys.mlkem1024_secret, &ml_ct_to_alice).unwrap();
        assert_eq!(ml_alice_encap, ml_bob_decap);
        assert_eq!(ml_bob_encap, ml_alice_decap);

        // === BUILD SHARED SECRETS STRUCT === low_* = from alice's perspective (alice is low handle) high_* = from bob's perspective (bob is high handle)
        //
        // For EC: both get same shared secret, but labeled by who initiated For KEM: low_* = alice→bob direction, high_* = bob→alice direction

        let alice_secrets = ClutchSharedSecrets {
            low_x25519: x25519_shared,
            high_x25519: x25519_shared, // Same for EC
            low_p384: p384_shared.clone(),
            high_p384: p384_shared.clone(),
            low_secp256k1: secp256k1_shared.clone(),
            high_secp256k1: secp256k1_shared.clone(),
            low_p256: p256_shared.clone(),
            high_p256: p256_shared.clone(),
            low_p521: p521_shared.clone(),
            high_p521: p521_shared.clone(),
            // KEM: directional
            low_frodo: frodo_ss_alice_encap.clone(), // Alice→Bob
            high_frodo: frodo_ss_alice_decap.clone(), // Bob→Alice (what Alice decapsulated)
            low_frodo1344: f13_alice_encap.clone(),
            high_frodo1344: f13_alice_decap.clone(),
            low_ntru: ntru_ss_alice_encap.clone(),
            high_ntru: ntru_ss_alice_decap.clone(),
            low_sntrup: sn_alice_encap.clone(),
            high_sntrup: sn_alice_decap.clone(),
            low_mlkem: ml_alice_encap.clone(),
            high_mlkem: ml_alice_decap.clone(),
            low_mceliece: mce_ss_alice_encap.clone(),
            high_mceliece: mce_ss_alice_decap.clone(),
            low_hqc: hqc_ss_alice_encap.clone(),
            high_hqc: hqc_ss_alice_decap.clone(),
        };

        let bob_secrets = ClutchSharedSecrets {
            low_x25519: x25519_shared,
            high_x25519: x25519_shared,
            low_p384: p384_shared.clone(),
            high_p384: p384_shared.clone(),
            low_secp256k1: secp256k1_shared.clone(),
            high_secp256k1: secp256k1_shared.clone(),
            low_p256: p256_shared.clone(),
            high_p256: p256_shared.clone(),
            low_p521: p521_shared.clone(),
            high_p521: p521_shared.clone(),
            // KEM: Bob's view is symmetric to Alice's
            low_frodo: frodo_ss_bob_decap.clone(), // Alice→Bob (what Bob decapsulated)
            high_frodo: frodo_ss_bob_encap.clone(), // Bob→Alice
            low_frodo1344: f13_bob_decap.clone(),
            high_frodo1344: f13_bob_encap.clone(),
            low_ntru: ntru_ss_bob_decap.clone(),
            high_ntru: ntru_ss_bob_encap.clone(),
            low_sntrup: sn_bob_decap.clone(),
            high_sntrup: sn_bob_encap.clone(),
            low_mlkem: ml_bob_decap.clone(),
            high_mlkem: ml_bob_encap.clone(),
            low_mceliece: mce_ss_bob_decap.clone(),
            high_mceliece: mce_ss_bob_encap.clone(),
            low_hqc: hqc_ss_bob_decap.clone(),
            high_hqc: hqc_ss_bob_encap.clone(),
        };

        // === COMPLETE CLUTCH ===
        let alice_result = clutch_complete_full(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &[0x5Au8; 32], // friendship secret — symmetric, same both sides
            &alice_secrets,
        );

        let bob_result = clutch_complete_full(
            &bob_device,
            &alice_device,
            &bob_handle,
            &alice_handle,
            &[0x5Au8; 32], // friendship secret — symmetric, same both sides
            &bob_secrets,
        );

        // === THE CRITICAL ASSERTIONS === Both parties should collect identical eggs
        assert_eq!(
            alice_result.eggs.eggs.len(),
            bob_result.eggs.eggs.len(),
            "egg count mismatch!"
        );
        for (i, (a, b)) in alice_result
            .eggs
            .eggs
            .iter()
            .zip(bob_result.eggs.eggs.iter())
            .enumerate()
        {
            assert_eq!(a, b, "egg {} mismatch!", i);
        }
        assert_eq!(alice_result.proof, bob_result.proof, "proof mismatch!");

        // Verify proof
        assert!(verify_eggs_proof(&alice_result.eggs, &bob_result.proof));

        // Cleanup
        alice_keys.zeroize();
        bob_keys.zeroize();
    }

    // ======================================================================== SPAGHETTIFY TESTS ========================================================================

    #[test]
    fn test_spaghettify_deterministic() {
        // Same input MUST produce same output
        let input = b"Hello, spaghetti world!";
        let output1 = spaghettify(input);
        let output2 = spaghettify(input);
        assert_eq!(output1, output2, "spaghettify must be deterministic");
    }

    #[test]
    fn test_spaghettify_empty_input() {
        // Empty input should produce valid output from LAVA_SEED
        let output = spaghettify(&[]);
        assert_ne!(
            output, [0u8; 32],
            "empty input should not produce all zeros"
        );

        // Should also be deterministic
        let output2 = spaghettify(&[]);
        assert_eq!(output, output2, "empty input should be deterministic");
    }

    #[test]
    fn test_spaghettify_different_inputs() {
        // Different inputs should produce different outputs
        let output1 = spaghettify(b"input one");
        let output2 = spaghettify(b"input two");
        assert_ne!(
            output1, output2,
            "different inputs should produce different outputs"
        );
    }

    #[test]
    fn test_spaghettify_avalanche() {
        // Flip one bit, should change ~50% of output bits
        let input1 = [0u8; 32];
        let mut input2 = [0u8; 32];
        input2[0] = 1; // Flip one bit

        let output1 = spaghettify(&input1);
        let output2 = spaghettify(&input2);

        // Count differing bits
        let mut diff_bits = 0;
        for (a, b) in output1.iter().zip(output2.iter()) {
            diff_bits += (a ^ b).count_ones();
        }

        // Should change roughly half the bits (128 ± 32 is reasonable)
        assert!(
            diff_bits > 64,
            "avalanche too weak: only {} bits changed",
            diff_bits
        );
        assert!(
            diff_bits < 192,
            "avalanche too strong: {} bits changed",
            diff_bits
        );
    }

    #[test]
    fn test_spaghettify_variable_rounds() {
        // Different inputs should trigger different round counts We can't directly verify round count, but we can verify different inputs produce different timing characteristics (not tested here) and that both produce valid outputs

        let short = spaghettify(&[0u8]);
        let long = spaghettify(&[255u8; 1000]);

        // Both should be valid 32-byte outputs
        assert_eq!(short.len(), 32);
        assert_eq!(long.len(), 32);
        // And different
        assert_ne!(short, long);
    }

    #[test]
    fn test_spaghettify_large_input() {
        // Should handle large inputs gracefully
        let large_input = vec![0xAB; 100_000]; // 100KB
        let output = spaghettify(&large_input);

        // Should produce valid output
        assert_eq!(output.len(), 32);
        assert_ne!(output, [0u8; 32]);

        // Should be deterministic
        let output2 = spaghettify(&large_input);
        assert_eq!(output, output2);
    }
}
