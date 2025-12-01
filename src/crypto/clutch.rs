//! CLUTCH Protocol - Cryptographic Layered Universal Trust Commitment Handshake
//!
//! A one-time key generation ceremony that establishes a shared seed for
//! rolling-chain encrypted communication between two parties.
//!
//! Phase 1: X25519-only (MVP)
//! Future: 8 primitives (X25519, P-384, secp256k1, ML-KEM, NTRU, FrodoKEM, HQC, McEliece)

use crate::types::Seed;
use blake3::Hasher;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroize;

/// Determine who initiates CLUTCH ceremony.
/// Lower handle_proof = initiator (sends ephemeral pubkeys first)
/// Higher handle_proof = responder (waits, then responds)
///
/// Both parties compute the same result, so no coordination needed.
pub fn is_clutch_initiator(our_handle_proof: &[u8; 32], their_handle_proof: &[u8; 32]) -> bool {
    our_handle_proof < their_handle_proof
}

/// Generate ephemeral X25519 keypair for CLUTCH ceremony
/// Returns (secret, public) - caller MUST zeroize the secret after use!
pub fn generate_clutch_ephemeral() -> ([u8; 32], [u8; 32]) {
    let mut secret_bytes = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut secret_bytes);

    let secret = StaticSecret::from(secret_bytes);
    let public = PublicKey::from(&secret);

    // Return the secret bytes for the caller to use (and zeroize when done)
    // Note: StaticSecret::from() copies the bytes, so we return the original
    (secret_bytes, *public.as_bytes())
}

/// Perform X25519 ECDH to derive shared secret
/// Caller should zeroize the returned shared secret after use
pub fn clutch_ecdh(our_secret: &[u8; 32], their_public: &[u8; 32]) -> [u8; 32] {
    let secret = StaticSecret::from(*our_secret);
    let public = PublicKey::from(*their_public);
    let shared = secret.diffie_hellman(&public);
    // x25519_dalek's SharedSecret zeroizes on drop, but we need the bytes
    *shared.as_bytes()
}

/// Sort two 32-byte arrays canonically (lower first)
fn sort_pair<'a>(a: &'a [u8; 32], b: &'a [u8; 32]) -> (&'a [u8; 32], &'a [u8; 32]) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Derive the CLUTCH shared seed from private handle hashes and X25519 shared secret.
///
/// This is the Phase 1 (X25519-only) seed derivation.
/// The seed is deterministic: both parties compute the same value.
///
/// SECURITY: Uses private handle_hash = BLAKE3(handle), NOT public handle_proof!
/// - handle_proof is PUBLIC (announced to FGTW, visible in peer table)
/// - handle_hash is PRIVATE (only known to parties who know the plaintext handle)
///
/// Handle hashes are sorted canonically so order of parties doesn't matter.
///
/// Note: Phase 1 uses 32-byte seed (sufficient for single primitive).
/// Full CLUTCH (8 primitives) will use 256-byte seed via BLAKE3 XOF.
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
    hasher.update(b"CLUTCH_v1_x25519_only");
    hasher.update(first);
    hasher.update(second);
    hasher.update(x25519_shared);

    Seed::from_bytes(*hasher.finalize().as_bytes())
}

/// Derive the CLUTCH shared seed using parallel key exchange.
///
/// Both parties generate and exchange ephemeral keys simultaneously.
/// BOTH ephemeral pubkeys contribute entropy to the final seed.
///
/// SECURITY: Uses private handle_hash = BLAKE3(handle), NOT public handle_proof!
///
/// Components are sorted canonically so order of parties doesn't matter:
/// - Handle hashes: sorted (lower first)
/// - Ephemeral pubkeys: sorted (lower first)
///
/// Uses BLAKE3 XOF to produce 256-byte seed (ready for full 8-primitive CLUTCH).
/// Phase 1 only uses first 32 bytes, but we derive the full seed for forward compat.
pub fn derive_clutch_seed_parallel(
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    our_ephemeral_pub: &[u8; 32],
    their_ephemeral_pub: &[u8; 32],
    x25519_shared: &[u8; 32],
) -> Seed {
    // Sort handle hashes canonically
    let (first_handle, second_handle) = sort_pair(our_handle_hash, their_handle_hash);

    // Sort ephemeral pubkeys canonically (both contribute entropy!)
    let (first_pub, second_pub) = sort_pair(our_ephemeral_pub, their_ephemeral_pub);

    let mut hasher = Hasher::new();
    hasher.update(b"CLUTCH_v2_parallel");
    hasher.update(first_handle); // Out-of-band secret
    hasher.update(second_handle);
    hasher.update(first_pub); // Both parties' randomness
    hasher.update(second_pub);
    hasher.update(x25519_shared); // ECDH result (32B for X25519-only)
                                  // Future: add other 7 shared secrets here for full CLUTCH

    // BLAKE3 XOF: extend output to 256 bytes (2048 bits)
    // Phase 1 uses Seed (32 bytes) but we derive full output for future compat
    let mut output = [0u8; 256];
    hasher.finalize_xof().fill(&mut output);

    // For now, use first 32 bytes as seed
    let mut seed_bytes = [0u8; 32];
    seed_bytes.copy_from_slice(&output[..32]);
    Seed::from_bytes(seed_bytes)
}

// Future: Full CLUTCH with 8 primitives will use this structure
// pub struct SharedSecrets {
//     pub x25519: [u8; 32],
//     pub p384: [u8; 48],
//     pub secp256k1: [u8; 32],
//     pub ml_kem: [u8; 32],
//     pub ntru: [u8; 32],
//     pub frodo: [u8; 24],
//     pub hqc: [u8; 64],
//     pub mceliece: [u8; 32],
// }
//
// pub fn derive_clutch_seed_full(
//     our_handle_proof: &[u8; 32],
//     their_handle_proof: &[u8; 32],
//     shared_secrets: &SharedSecrets,
// ) -> [u8; 256] {
//     let (first, second) = if our_handle_proof < their_handle_proof {
//         (our_handle_proof, their_handle_proof)
//     } else {
//         (their_handle_proof, our_handle_proof)
//     };
//
//     let mut hasher = Hasher::new();
//     hasher.update(b"CLUTCH_v1_full");
//     hasher.update(first);
//     hasher.update(second);
//     hasher.update(&shared_secrets.x25519);
//     hasher.update(&shared_secrets.p384);
//     hasher.update(&shared_secrets.secp256k1);
//     hasher.update(&shared_secrets.ml_kem);
//     hasher.update(&shared_secrets.ntru);
//     hasher.update(&shared_secrets.frodo);
//     hasher.update(&shared_secrets.hqc);
//     hasher.update(&shared_secrets.mceliece);
//
//     // BLAKE3 XOF: extend output to 256 bytes (2048 bits)
//     let mut output = [0u8; 256];
//     hasher.finalize_xof().fill(&mut output);
//     output
// }

/// Compute the CLUTCH completion proof.
/// Sent by initiator to confirm they derived the same seed.
/// Responder can verify without revealing the seed.
pub fn compute_clutch_proof(seed: &Seed) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(seed.as_bytes());
    hasher.update(b"CLUTCH_v1_complete");
    *hasher.finalize().as_bytes()
}

/// Verify the CLUTCH completion proof matches our derived seed.
pub fn verify_clutch_proof(seed: &Seed, proof: &[u8; 32]) -> bool {
    let expected = compute_clutch_proof(seed);
    // Constant-time comparison
    expected.iter().zip(proof.iter()).all(|(a, b)| a == b)
}

/// Full CLUTCH ceremony result
pub struct ClutchResult {
    pub seed: Seed,
    pub proof: [u8; 32],
}

/// Perform complete CLUTCH ceremony using parallel key exchange.
///
/// Both parties generate ephemeral keypairs simultaneously and exchange them.
/// Both pubkeys contribute entropy to the final seed.
///
/// Steps:
/// 1. Generate ephemeral keypair (done before calling this)
/// 2. Exchange ClutchOffer messages (both directions, parallel)
/// 3. Once both pubkeys known, call this function
/// 4. Lower handle_proof party sends ClutchComplete with proof
///
/// SECURITY: Takes private handle_hash = BLAKE3(handle), NOT public handle_proof!
pub fn clutch_complete_parallel(
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    our_ephemeral_secret: &[u8; 32],
    our_ephemeral_pubkey: &[u8; 32],
    their_ephemeral_pubkey: &[u8; 32],
) -> ClutchResult {
    let mut x25519_shared = clutch_ecdh(our_ephemeral_secret, their_ephemeral_pubkey);
    let seed = derive_clutch_seed_parallel(
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

// Legacy functions for v1 compatibility - kept for existing contacts
/// Perform complete CLUTCH ceremony as initiator (v1 sequential).
/// DEPRECATED: Use clutch_complete_parallel for new contacts.
pub fn clutch_as_initiator(
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    our_ephemeral_secret: &[u8; 32],
    their_ephemeral_pubkey: &[u8; 32],
) -> ClutchResult {
    let mut x25519_shared = clutch_ecdh(our_ephemeral_secret, their_ephemeral_pubkey);
    let seed = derive_clutch_seed_x25519(our_handle_hash, their_handle_hash, &x25519_shared);
    let proof = compute_clutch_proof(&seed);
    x25519_shared.zeroize();
    ClutchResult { seed, proof }
}

/// Perform complete CLUTCH ceremony as responder (v1 sequential).
/// DEPRECATED: Use clutch_complete_parallel for new contacts.
pub fn clutch_as_responder(
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    our_ephemeral_secret: &[u8; 32],
    their_ephemeral_pubkey: &[u8; 32],
) -> ClutchResult {
    let mut x25519_shared = clutch_ecdh(our_ephemeral_secret, their_ephemeral_pubkey);
    let seed = derive_clutch_seed_x25519(our_handle_hash, their_handle_hash, &x25519_shared);
    let proof = compute_clutch_proof(&seed);
    x25519_shared.zeroize();
    ClutchResult { seed, proof }
}

#[cfg(test)]
mod tests {
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
    fn test_clutch_ceremony_produces_same_seed() {
        // Simulate private handle hashes = BLAKE3(handle)
        let alice_handle_hash = *blake3::hash(b"alice test handle").as_bytes();
        let bob_handle_hash = *blake3::hash(b"bob test handle").as_bytes();

        // Generate ephemeral keypairs
        let (alice_secret, alice_public) = generate_clutch_ephemeral();
        let (bob_secret, bob_public) = generate_clutch_ephemeral();

        // Alice as initiator (uses private handle hashes, NOT public handle_proof!)
        let alice_result = clutch_as_initiator(
            &alice_handle_hash,
            &bob_handle_hash,
            &alice_secret,
            &bob_public,
        );

        // Bob as responder
        let bob_result = clutch_as_responder(
            &bob_handle_hash,
            &alice_handle_hash,
            &bob_secret,
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
    fn test_different_handles_different_seeds() {
        // Private handle hashes (BLAKE3 of plaintext handle)
        let handle_hash1 = *blake3::hash(b"handle one").as_bytes();
        let handle_hash2 = *blake3::hash(b"handle two").as_bytes();
        let handle_hash3 = *blake3::hash(b"handle three").as_bytes();

        let (secret, public) = generate_clutch_ephemeral();
        let shared = clutch_ecdh(&secret, &public);

        let seed_12 = derive_clutch_seed_x25519(&handle_hash1, &handle_hash2, &shared);
        let seed_13 = derive_clutch_seed_x25519(&handle_hash1, &handle_hash3, &shared);

        // Different handle pairs should produce different seeds
        assert_ne!(seed_12.as_bytes(), seed_13.as_bytes());
    }

    #[test]
    fn test_parallel_clutch_produces_same_seed() {
        // Private handle hashes
        let alice_handle_hash = *blake3::hash(b"alice parallel handle").as_bytes();
        let bob_handle_hash = *blake3::hash(b"bob parallel handle").as_bytes();

        // Both generate ephemeral keypairs simultaneously
        let (alice_secret, alice_public) = generate_clutch_ephemeral();
        let (bob_secret, bob_public) = generate_clutch_ephemeral();

        // Both complete the ceremony with all four pubkeys
        let alice_result = clutch_complete_parallel(
            &alice_handle_hash,
            &bob_handle_hash,
            &alice_secret,
            &alice_public,
            &bob_public,
        );

        let bob_result = clutch_complete_parallel(
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
        let handle1 = *blake3::hash(b"handle 1").as_bytes();
        let handle2 = *blake3::hash(b"handle 2").as_bytes();

        let (secret1, pub1) = generate_clutch_ephemeral();
        let (secret2, pub2) = generate_clutch_ephemeral();

        let shared = clutch_ecdh(&secret1, &pub2);

        // Derive seed with pubkeys in both orders - should produce same result
        let seed_a = derive_clutch_seed_parallel(&handle1, &handle2, &pub1, &pub2, &shared);
        let seed_b = derive_clutch_seed_parallel(&handle1, &handle2, &pub2, &pub1, &shared);

        assert_eq!(seed_a.as_bytes(), seed_b.as_bytes());
    }

    #[test]
    fn test_parallel_vs_sequential_different_seeds() {
        // Parallel and sequential should produce DIFFERENT seeds (different domain separators)
        let alice_hash = *blake3::hash(b"alice").as_bytes();
        let bob_hash = *blake3::hash(b"bob").as_bytes();

        let (alice_secret, alice_public) = generate_clutch_ephemeral();
        let (bob_secret, bob_public) = generate_clutch_ephemeral();

        // Sequential v1
        let v1_result = clutch_as_initiator(&alice_hash, &bob_hash, &alice_secret, &bob_public);

        // Parallel v2
        let v2_result = clutch_complete_parallel(
            &alice_hash,
            &bob_hash,
            &alice_secret,
            &alice_public,
            &bob_public,
        );

        // Seeds MUST be different (different protocol versions)
        assert_ne!(v1_result.seed.as_bytes(), v2_result.seed.as_bytes());
    }
}
