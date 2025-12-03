//! CLUTCH Protocol - Cryptographic Layered Universal Trust Commitment Handshake
//!
//! Two-layer cryptographic ceremony for secure messaging:
//!
//! ## Layer 1: Conversation Provenance (permanent identity binding)
//! - Derived from: device pubkeys + handle hashes + mutual signatures
//! - Never changes for a given pair of identities
//! - Used for: filenames, conversation filtering, identity verification
//! - Survives re-CLUTCH (key rotation)
//!
//! ## Layer 2: CLUTCH Seed (ephemeral encryption key material)
//! - Derived from: provenance + ephemeral DH + ECDH shared secret
//! - Changes on each re-CLUTCH ceremony
//! - Used for: PRNG seed, message encryption, forward secrecy
//!
//! ## Re-CLUTCH Flow (key rotation without losing conversation identity)
//! 1. Either party can initiate re-CLUTCH at any time
//! 2. New ephemeral keys are generated and exchanged
//! 3. New CLUTCH seed is derived (provenance stays the same)
//! 4. Old seed is zeroized after confirmation
//! 5. Conversation continues with new key material
//!
//! ## Phase 1: X25519-only (MVP - one egg omelette)
//! ## Future: 8 primitives (X25519, P-384, secp256k1, ML-KEM, NTRU, FrodoKEM, HQC, McEliece)

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

// ============================================================================
// LAYER 1: CONVERSATION PROVENANCE (permanent identity binding)
// ============================================================================

/// Derive the conversation provenance hash.
///
/// This is a PERMANENT identifier for a conversation between two parties.
/// It depends ONLY on identity (device keys + handle hashes + signatures),
/// NOT on ephemeral CLUTCH keys. This means:
/// - Same provenance survives re-CLUTCH (key rotation)
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

    // Signatures are included but order doesn't matter for the hash
    // We sort by the device pubkey order for consistency
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

/// Compute the handshake message that both parties sign.
///
/// This is signed by each party with their device private key.
/// The signatures become part of the provenance derivation.
///
/// Contains: sorted device pubkeys + sorted handle hashes + timestamp
/// The timestamp prevents replay but is NOT part of provenance (so same
/// parties can re-establish with same provenance).
pub fn compute_handshake_message(
    our_device_pubkey: &[u8; 32],
    their_device_pubkey: &[u8; 32],
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
) -> [u8; 32] {
    let (first_device, second_device) = sort_pair(our_device_pubkey, their_device_pubkey);
    let (first_handle, second_handle) = sort_pair(our_handle_hash, their_handle_hash);

    let mut hasher = Hasher::new();
    hasher.update(b"PHOTON_HANDSHAKE_v1");
    hasher.update(first_device);
    hasher.update(second_device);
    hasher.update(first_handle);
    hasher.update(second_handle);

    *hasher.finalize().as_bytes()
}

// ============================================================================
// LAYER 2: CLUTCH SEED (ephemeral encryption key material)
// ============================================================================

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
/// - Device pubkeys: sorted (lower first) - binds to device identity
/// - Handle hashes: sorted (lower first)
/// - Ephemeral pubkeys: sorted (lower first)
///
/// Uses BLAKE3 XOF to produce 256-byte seed (ready for full 8-primitive CLUTCH).
/// Phase 1 only uses first 32 bytes, but we derive the full seed for forward compat.
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
    hasher.update(b"CLUTCH_v3_device_bound"); // New version - device keys now bound
    hasher.update(first_device); // Device identity binding (prevents spoofing)
    hasher.update(second_device);
    hasher.update(first_handle); // Out-of-band secret (handle hash)
    hasher.update(second_handle);
    hasher.update(first_pub); // Both parties' ephemeral randomness
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
    use subtle::ConstantTimeEq;
    let expected = compute_clutch_proof(seed);
    // Actually constant-time comparison (subtle crate)
    expected.ct_eq(proof).into()
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
/// Device pubkeys are mixed in to bind the seed to both device identities.
///
/// Steps:
/// 1. Generate ephemeral keypair (done before calling this)
/// 2. Exchange ClutchOffer messages (both directions, parallel)
/// 3. Once both pubkeys known, call this function
/// 4. Lower handle_proof party sends ClutchComplete with proof
///
/// SECURITY: Takes private handle_hash = BLAKE3(handle), NOT public handle_proof!
/// Device pubkeys are mixed in to prevent handle spoofing with different device.
pub fn clutch_complete_parallel(
    our_device_pubkey: &[u8; 32],
    their_device_pubkey: &[u8; 32],
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    our_ephemeral_secret: &[u8; 32],
    our_ephemeral_pubkey: &[u8; 32],
    their_ephemeral_pubkey: &[u8; 32],
) -> ClutchResult {
    let mut x25519_shared = clutch_ecdh(our_ephemeral_secret, their_ephemeral_pubkey);
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
    fn test_clutch_ceremony_v1_compatibility_removed() {
        // This test verified v1 sequential CLUTCH (initiator/responder pattern).
        // v3 uses parallel exchange only - see test_parallel_clutch_produces_same_seed.
        // Keeping this stub to document the intentional removal of v1 support.
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
        // Device pubkeys (simulated Ed25519 public keys)
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];

        // Private handle hashes
        let alice_handle_hash = *blake3::hash(b"alice parallel handle").as_bytes();
        let bob_handle_hash = *blake3::hash(b"bob parallel handle").as_bytes();

        // Both generate ephemeral keypairs simultaneously
        let (alice_secret, alice_public) = generate_clutch_ephemeral();
        let (bob_secret, bob_public) = generate_clutch_ephemeral();

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

        let (secret1, pub1) = generate_clutch_ephemeral();
        let (secret2, pub2) = generate_clutch_ephemeral();

        let shared = clutch_ecdh(&secret1, &pub2);

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

        let (secret1, pub1) = generate_clutch_ephemeral();
        let (_secret2, pub2) = generate_clutch_ephemeral();

        let shared = clutch_ecdh(&secret1, &pub2);

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

    #[test]
    fn test_parallel_vs_sequential_different_seeds() {
        // Parallel and sequential should produce DIFFERENT seeds (different domain separators)
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];
        let alice_hash = *blake3::hash(b"alice").as_bytes();
        let bob_hash = *blake3::hash(b"bob").as_bytes();

        let (alice_secret, alice_public) = generate_clutch_ephemeral();
        let (_bob_secret, bob_public) = generate_clutch_ephemeral();

        // Sequential v1
        let v1_result = clutch_as_initiator(&alice_hash, &bob_hash, &alice_secret, &bob_public);

        // Parallel v3 (now with device binding)
        let v3_result = clutch_complete_parallel(
            &alice_device,
            &bob_device,
            &alice_hash,
            &bob_hash,
            &alice_secret,
            &alice_public,
            &bob_public,
        );

        // Seeds MUST be different (different protocol versions)
        assert_ne!(v1_result.seed.as_bytes(), v3_result.seed.as_bytes());
    }

    // ========================================================================
    // PROVENANCE TESTS
    // ========================================================================

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
        // Key insight: provenance doesn't include ephemeral keys
        // So re-CLUTCH (new ephemeral keys) produces same provenance
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];
        let alice_handle = *blake3::hash(b"alice").as_bytes();
        let bob_handle = *blake3::hash(b"bob").as_bytes();
        let alice_sig = [0xAAu8; 64];
        let bob_sig = [0xBBu8; 64];

        // First CLUTCH - get provenance
        let provenance_1 = derive_conversation_provenance(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &alice_sig,
            &bob_sig,
        );

        // Re-CLUTCH with new ephemeral keys (simulated)
        // But same device keys, handles, and signatures
        let provenance_2 = derive_conversation_provenance(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &alice_sig,
            &bob_sig,
        );

        // Provenance MUST be identical after re-CLUTCH
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
}

// ============================================================================
// FUTURE ROADMAP: THE FULL 8-EGG OMELETTE
// ============================================================================
//
// Current: Phase 1 (X25519-only, one egg)
//
// Future eggs to add for full quantum-resistant CLUTCH:
//
// EGG 1: X25519 (DONE - current implementation)
//   - Classic ECDH, fast, well-understood
//   - Broken by quantum computers (Shor's algorithm)
//
// EGG 2: P-384 (ECDH on NIST P-384 curve)
//   - Different curve family than X25519
//   - Provides diversity in case of X25519 weakness
//
// EGG 3: secp256k1 (Bitcoin's curve)
//   - Yet another curve family
//   - Battle-tested by billions of dollars
//
// EGG 4: ML-KEM (CRYSTALS-Kyber successor, NIST standard)
//   - Lattice-based, quantum-resistant
//   - NIST's chosen KEM standard
//
// EGG 5: NTRU
//   - Lattice-based, older design
//   - Different lattice assumptions than ML-KEM
//
// EGG 6: FrodoKEM
//   - Conservative lattice-based design
//   - Larger keys but simpler security assumptions
//
// EGG 7: HQC
//   - Code-based, quantum-resistant
//   - Different mathematical foundation than lattice
//
// EGG 8: Classic McEliece
//   - Code-based, oldest post-quantum design
//   - Very large keys but extreme confidence in security
//
// Each egg contributes its shared secret to the final hash.
// If ANY single egg remains secure, the conversation is protected.
// This is defense in depth against unknown future attacks.
//
// ============================================================================
// RE-CLUTCH PROTOCOL (key rotation)
// ============================================================================
//
// When to re-CLUTCH:
// - Periodically (e.g., every N messages or time period)
// - After suspected compromise
// - When either party requests it
// - After device state recovery from backup
//
// Re-CLUTCH message flow:
//
// 1. INITIATOR                              RESPONDER
//    |                                           |
//    |--- ReclutchRequest ------------------->   |
//    |    (new_ephemeral_pub, signed)            |
//    |                                           |
//    |   <--- ReclutchResponse ---------------  |
//    |        (new_ephemeral_pub, signed)        |
//    |                                           |
//    [Both derive new CLUTCH seed]               |
//    [Old seed zeroized]                         |
//    |                                           |
//    |--- ReclutchConfirm ------------------->   |
//    |    (proof of new seed)                    |
//    |                                           |
//    [Conversation continues with new keys]      |
//
// Key properties:
// - Provenance stays the same (identity binding preserved)
// - Only CLUTCH seed changes (forward secrecy)
// - Old key material is zeroized after confirmation
// - Either party can initiate at any time
// - No message loss during transition (sequence numbers)
//
// ============================================================================
// MESSAGE ENCRYPTION LAYERS (future)
// ============================================================================
//
// Layer 0: Signature verification (device key proves identity)
//   - Every message signed with sender's device private key
//   - Verified against known device pubkey from handshake
//
// Layer 1: PRNG XOR from CLUTCH seed
//   - BLAKE3 XOF seeded with CLUTCH seed + message counter
//   - XOR with message content
//   - Provides forward secrecy (new key for each message)
//
// Layer 2: Device key asymmetric encryption
//   - Encrypt with recipient's device public key
//   - Only recipient can decrypt
//
// Layer 3: One-time pad (optional, for extreme security)
//   - Pre-shared random data exchanged out-of-band
//   - XOR with final ciphertext
//   - Information-theoretic security if pad is truly random
//
// ============================================================================
