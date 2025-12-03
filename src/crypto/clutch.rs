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

// ============================================================================
// CLASS 0: CLASSICAL ELLIPTIC CURVES
// ============================================================================

/// Generate P-384 ephemeral keypair
/// Returns (secret_bytes, public_bytes)
pub fn generate_p384_ephemeral() -> (Vec<u8>, Vec<u8>) {
    use p384::SecretKey;

    let secret = SecretKey::generate();
    let public = secret.public_key();

    let secret_bytes = secret.to_bytes().to_vec();
    let public_bytes = public.to_sec1_bytes().to_vec();

    (secret_bytes, public_bytes)
}

/// Perform P-384 ECDH
/// Returns 48-byte shared secret
pub fn p384_ecdh(our_secret: &[u8], their_public: &[u8]) -> Vec<u8> {
    use p384::elliptic_curve::ecdh::diffie_hellman;
    use p384::{PublicKey, SecretKey};

    let secret = SecretKey::from_slice(our_secret).expect("P-384 secret key invalid");
    let public = PublicKey::from_sec1_bytes(their_public).expect("P-384 public key invalid");

    let shared = diffie_hellman(secret.to_nonzero_scalar(), public.as_affine());
    shared.raw_secret_bytes().to_vec()
}

/// Generate secp256k1 ephemeral keypair
/// Returns (secret_bytes, public_bytes)
pub fn generate_secp256k1_ephemeral() -> (Vec<u8>, Vec<u8>) {
    use k256::SecretKey;

    let secret = SecretKey::generate();
    let public = secret.public_key();

    let secret_bytes = secret.to_bytes().to_vec();
    let public_bytes = public.to_sec1_bytes().to_vec();

    (secret_bytes, public_bytes)
}

/// Perform secp256k1 ECDH
/// Returns 32-byte shared secret
pub fn secp256k1_ecdh(our_secret: &[u8], their_public: &[u8]) -> Vec<u8> {
    use k256::elliptic_curve::ecdh::diffie_hellman;
    use k256::{PublicKey, SecretKey};

    let secret = SecretKey::from_slice(our_secret).expect("secp256k1 secret key invalid");
    let public = PublicKey::from_sec1_bytes(their_public).expect("secp256k1 public key invalid");

    let shared = diffie_hellman(secret.to_nonzero_scalar(), public.as_affine());
    shared.raw_secret_bytes().to_vec()
}

/// Generate P-256 ephemeral keypair
/// Returns (secret_bytes, public_bytes)
pub fn generate_p256_ephemeral() -> (Vec<u8>, Vec<u8>) {
    use p256::SecretKey;

    let secret = SecretKey::generate();
    let public = secret.public_key();

    let secret_bytes = secret.to_bytes().to_vec();
    let public_bytes = public.to_sec1_bytes().to_vec();

    (secret_bytes, public_bytes)
}

/// Perform P-256 ECDH
/// Returns 32-byte shared secret
pub fn p256_ecdh(our_secret: &[u8], their_public: &[u8]) -> Vec<u8> {
    use p256::elliptic_curve::ecdh::diffie_hellman;
    use p256::{PublicKey, SecretKey};

    let secret = SecretKey::from_slice(our_secret).expect("P-256 secret key invalid");
    let public = PublicKey::from_sec1_bytes(their_public).expect("P-256 public key invalid");

    let shared = diffie_hellman(secret.to_nonzero_scalar(), public.as_affine());
    shared.raw_secret_bytes().to_vec()
}

// ============================================================================
// CLASS 1: POST-QUANTUM LATTICE KEMS
// ============================================================================

/// Generate FrodoKEM-976-SHAKE keypair
/// Returns (secret_key, public_key)
pub fn generate_frodo976_keypair() -> (Vec<u8>, Vec<u8>) {
    use frodo_kem_rs::Algorithm;
    use rand_core::OsRng;

    let alg = Algorithm::FrodoKem976Shake;
    let (ek, dk) = alg
        .try_generate_keypair(OsRng)
        .expect("FrodoKEM keygen failed");

    (dk.value().to_vec(), ek.value().to_vec())
}

/// Encapsulate FrodoKEM-976-SHAKE
/// Returns (ciphertext, shared_secret)
pub fn frodo976_encapsulate(their_public_key: &[u8]) -> (Vec<u8>, Vec<u8>) {
    use frodo_kem_rs::{Algorithm, EncryptionKey};
    use rand_core::OsRng;

    let alg = Algorithm::FrodoKem976Shake;
    let ek =
        EncryptionKey::from_bytes(alg, their_public_key).expect("FrodoKEM pubkey parse failed");
    let (ct, ss) = alg
        .try_encapsulate_with_rng(&ek, OsRng)
        .expect("FrodoKEM encapsulate failed");

    (ct.value().to_vec(), ss.value().to_vec())
}

/// Decapsulate FrodoKEM-976-SHAKE
/// Returns shared_secret
pub fn frodo976_decapsulate(our_secret_key: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    use frodo_kem_rs::{Algorithm, Ciphertext, DecryptionKey};

    let alg = Algorithm::FrodoKem976Shake;
    let dk = DecryptionKey::from_bytes(alg, our_secret_key).expect("FrodoKEM seckey parse failed");
    let ct = Ciphertext::from_bytes(alg, ciphertext).expect("FrodoKEM ciphertext parse failed");
    let (ss, _msg) = alg
        .decapsulate(&dk, &ct)
        .expect("FrodoKEM decapsulate failed");

    ss.value().to_vec()
}

/// Generate NTRU-HRSS-701 keypair
/// Returns (secret_key, public_key)
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

/// Encapsulate NTRU-HRSS-701
/// Returns (ciphertext, 32B shared_secret)
pub fn ntru701_encapsulate(their_public_key: &[u8]) -> (Vec<u8>, Vec<u8>) {
    use pqcrypto_ntru::ntruhrss701;
    use pqcrypto_traits::kem::{Ciphertext, PublicKey, SharedSecret};

    let pk = <ntruhrss701::PublicKey as PublicKey>::from_bytes(their_public_key)
        .expect("NTRU public key invalid");

    // NTRU uses its own internal RNG for encapsulation
    let (ss, ct) = ntruhrss701::encapsulate(&pk);

    (
        Ciphertext::as_bytes(&ct).to_vec(),
        SharedSecret::as_bytes(&ss).to_vec(),
    )
}

/// Decapsulate NTRU-HRSS-701
/// Returns 32B shared_secret
pub fn ntru701_decapsulate(our_secret_key: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    use pqcrypto_ntru::ntruhrss701;
    use pqcrypto_traits::kem::{Ciphertext, SecretKey, SharedSecret};

    let sk = <ntruhrss701::SecretKey as SecretKey>::from_bytes(our_secret_key)
        .expect("NTRU secret key invalid");
    let ct = <ntruhrss701::Ciphertext as Ciphertext>::from_bytes(ciphertext)
        .expect("NTRU ciphertext invalid");

    let ss = ntruhrss701::decapsulate(&ct, &sk);

    SharedSecret::as_bytes(&ss).to_vec()
}

// ============================================================================
// CLASS 2: POST-QUANTUM CODE-BASED KEMS
// ============================================================================

/// Generate Classic McEliece 460896 keypair
/// Returns (secret_key, public_key ~512KB)
pub fn generate_mceliece460896_keypair() -> (Vec<u8>, Vec<u8>) {
    use classic_mceliece_rust::keypair_boxed;

    // McEliece uses a different RNG - use rng for diversity
    let mut rng = rand::thread_rng();
    let (pk, sk) = keypair_boxed(&mut rng);

    (sk.as_array().to_vec(), pk.as_array().to_vec())
}

/// Encapsulate Classic McEliece 460896
/// Returns (ciphertext, 32B shared_secret)
pub fn mceliece460896_encapsulate(their_public_key: &[u8]) -> (Vec<u8>, Vec<u8>) {
    use classic_mceliece_rust::{encapsulate_boxed, PublicKey, CRYPTO_PUBLICKEYBYTES};

    // Copy to Box for PublicKey::from
    let mut pk_box = vec![0u8; CRYPTO_PUBLICKEYBYTES].into_boxed_slice();
    pk_box.copy_from_slice(their_public_key);
    let pk_array: Box<[u8; CRYPTO_PUBLICKEYBYTES]> =
        pk_box.try_into().expect("McEliece public key wrong size");
    let pk = PublicKey::from(pk_array);

    // Use rng for McEliece encapsulation (another diverse RNG source)
    let mut rng = rand::thread_rng();
    let (ss, ct) = encapsulate_boxed(&pk, &mut rng);

    (ct.as_array().to_vec(), ss.as_array().to_vec())
}

/// Decapsulate Classic McEliece 460896
/// Returns 32B shared_secret
pub fn mceliece460896_decapsulate(our_secret_key: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    use classic_mceliece_rust::{
        decapsulate_boxed, Ciphertext, SecretKey, CRYPTO_CIPHERTEXTBYTES, CRYPTO_SECRETKEYBYTES,
    };

    // Copy to Box for SecretKey::from
    let mut sk_box = vec![0u8; CRYPTO_SECRETKEYBYTES].into_boxed_slice();
    sk_box.copy_from_slice(our_secret_key);
    let sk_array: Box<[u8; CRYPTO_SECRETKEYBYTES]> =
        sk_box.try_into().expect("McEliece secret key wrong size");
    let sk = SecretKey::from(sk_array);

    // Copy to array for Ciphertext::from
    let ct_array: [u8; CRYPTO_CIPHERTEXTBYTES] = ciphertext
        .try_into()
        .expect("McEliece ciphertext wrong size");
    let ct = Ciphertext::from(ct_array);

    let ss = decapsulate_boxed(&ct, &sk);

    ss.as_array().to_vec()
}

/// Generate HQC-256 keypair
/// Returns (secret_key, public_key)
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

/// Encapsulate HQC-256
/// Returns (ciphertext, 64B shared_secret)
pub fn hqc256_encapsulate(their_public_key: &[u8]) -> (Vec<u8>, Vec<u8>) {
    use pqcrypto_hqc::hqc256;
    use pqcrypto_traits::kem::{Ciphertext, PublicKey, SharedSecret};

    let pk = <hqc256::PublicKey as PublicKey>::from_bytes(their_public_key)
        .expect("HQC public key invalid");

    // HQC uses its own internal RNG for encapsulation
    let (ss, ct) = hqc256::encapsulate(&pk);

    (
        Ciphertext::as_bytes(&ct).to_vec(),
        SharedSecret::as_bytes(&ss).to_vec(),
    )
}

/// Decapsulate HQC-256
/// Returns 64B shared_secret
pub fn hqc256_decapsulate(our_secret_key: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    use pqcrypto_hqc::hqc256;
    use pqcrypto_traits::kem::{Ciphertext, SecretKey, SharedSecret};

    let sk = <hqc256::SecretKey as SecretKey>::from_bytes(our_secret_key)
        .expect("HQC secret key invalid");
    let ct =
        <hqc256::Ciphertext as Ciphertext>::from_bytes(ciphertext).expect("HQC ciphertext invalid");

    let ss = hqc256::decapsulate(&ct, &sk);

    SharedSecret::as_bytes(&ss).to_vec()
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

/// All shared secrets from 12 cryptographic primitives.
/// Each "egg" is a labeled BLAKE3 hash for domain separation.
pub struct ClutchEggs {
    pub eggs: Vec<Vec<u8>>,
}

impl ClutchEggs {
    pub fn new() -> Self {
        ClutchEggs { eggs: Vec::new() }
    }

    /// Add an egg with domain-separated labeling
    fn add_egg(&mut self, label: &str, shared_secret: &[u8]) {
        let mut hasher = Hasher::new();
        hasher.update(b"CLUTCH_EGG_v4_");
        hasher.update(label.as_bytes());
        hasher.update(shared_secret);
        self.eggs.push(hasher.finalize().as_bytes().to_vec());
    }
}

/// Collect all 14 cryptographic primitive shared secrets as labeled eggs.
/// 6 context eggs + 8 KEMs (4 classical curves, 2 lattice, 2 code-based).
///
/// Each egg is a BLAKE3 hash with domain separation:
/// BLAKE3("CLUTCH_EGG_v4_" || label || shared_secret)
///
/// Returns vector of 14 eggs ready for avalanche hashing.
pub fn collect_clutch_eggs(
    our_device_pubkey: &[u8; 32],
    their_device_pubkey: &[u8; 32],
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    our_ephemeral_pub: &[u8; 32],
    their_ephemeral_pub: &[u8; 32],
    x25519_shared: &[u8; 32],
    p384_shared: &[u8],
    secp256k1_shared: &[u8],
    frodo_shared: &[u8],
    ntru_shared: &[u8],
    mceliece_shared: &[u8],
    hqc_shared: &[u8],
    p256_shared: &[u8],
) -> ClutchEggs {
    let mut eggs = ClutchEggs::new();

    eggs.add_egg("our pubkey", our_device_pubkey);
    eggs.add_egg("their pubkey", their_device_pubkey);
    eggs.add_egg("our handle hash", our_handle_hash);
    eggs.add_egg("their handle hash", their_handle_hash);
    eggs.add_egg("our ephemeral pub", our_ephemeral_pub);
    eggs.add_egg("their ephemeral pub", their_ephemeral_pub);

    eggs.add_egg("x25519", x25519_shared);
    eggs.add_egg("p384", p384_shared);
    eggs.add_egg("secp256k1", secp256k1_shared);

    eggs.add_egg("frodo976", frodo_shared);
    eggs.add_egg("ntru701", ntru_shared);

    eggs.add_egg("mceliece460896", mceliece_shared);
    eggs.add_egg("hqc256", hqc_shared);
    eggs.add_egg("p256", p256_shared);

    eggs
}

/// Avalanche hash the eggs into a 1MB mixing pad for conversation state.
///
/// This is a memory-hard, deterministic mixing function that:
/// 1. Flattens all 14 eggs into a single buffer (448 bytes)
/// 2. Repeatedly copies pseudo-random chunks to grow >= 1MB
/// 3. Heavy mixing with diverse operations (+, -, *, ^, %, <<, >>)
/// 4. Trims to exactly 1MB
///
/// Properties:
/// - Deterministic: same eggs → same pad
/// - Memory-hard: 1MB final state
/// - Avalanche: every bit of input affects every bit of output
/// - Diverse operations: prevents algebraic attacks
///
/// The returned pad is saved locally and rotated with message hashes.
/// Seed derivation: BLAKE3(pad) whenever a key is needed.
///
/// Returns the 1MB pad (Vec<u8>) for conversation state.
pub fn avalanche_hash_eggs(eggs: &ClutchEggs) -> Vec<u8> {
    use i256::U256;

    const MIN_SIZE: usize = 1_048_576;
    let max_size = MIN_SIZE * 2;

    // Step 1: Flatten all eggs into one buffer
    let mut omelette = Vec::with_capacity(max_size);
    for egg in &eggs.eggs {
        omelette.extend_from_slice(egg);
    }

    let mut target_hasher = Hasher::new();
    target_hasher.update(b"target");
    target_hasher.update(&omelette);
    let target_hash = target_hasher.finalize();
    let target_u256 = U256::from_be_bytes(*target_hash.as_bytes());
    let target_size = MIN_SIZE + (target_u256 % U256::from(MIN_SIZE as u128)).as_u128() as usize;

    // Step 2: Grow to 1MB by copying pseudo-random chunks
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

        // Copy chunk [start..stop] and append/prepend based on direction
        let chunk = omelette[start..stop].to_vec();
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

    // Step 3: Heavy mixing with diverse operations
    // Process as variable-sized chunks (1-32 bytes, unaligned) for maximum diffusion
    const MIX_ROUNDS: usize = 8;

    for round in 0..MIX_ROUNDS {
        let len = omelette.len();

        // Hash current state to derive mixing parameters
        let mut round_hasher = Hasher::new();
        round_hasher.update(&omelette);
        round_hasher.update(&[round as u8]);
        let round_hash = round_hasher.finalize();
        let round_u256 = U256::from_be_bytes(*round_hash.as_bytes());

        // Determine chunk size for this round (1-32 bytes, unaligned)
        let chunk_size = 1 + ((round_u256 % U256::from(32_u128)).as_u128() as usize);

        // Mix chunks with diverse operations
        let num_chunks = len / chunk_size;

        for i in 0..num_chunks {
            let pos = i * chunk_size;
            if pos + chunk_size > len {
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
            let idx1 = ((idx_u256 % U256::from(num_chunks as u128)).as_u128() as usize) * chunk_size;
            let idx2 = (((idx_u256 >> 64_u32) % U256::from(num_chunks as u128)).as_u128() as usize) * chunk_size;

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
                    0 => val.wrapping_add(v1) ^ v2,                    // + and ^
                    1 => val.wrapping_sub(v1).wrapping_mul(v2),        // - and *
                    2 => (val ^ v1).wrapping_add(v2),                  // ^ and +
                    3 => val.wrapping_mul(0xEF) ^ v1 ^ 0xBE,          // DEADBEEF nibbles
                    4 => (val << (v1 & 7)) ^ v2,                       // << shift
                    5 => (val >> (v2 & 7)) ^ v1,                       // >> shift
                    6 => val ^ v1 ^ v2 ^ 0xDE,                         // More DEADBEEF
                    _ => val,
                };
            }
        }
    }

    // Step 4: Trim to exactly 1MB
    omelette.truncate(MIN_SIZE);

    omelette
}

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

    #[test]
    fn test_egg_collection_produces_vector() {
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];
        let alice_handle = *blake3::hash(b"alice").as_bytes();
        let bob_handle = *blake3::hash(b"bob").as_bytes();
        let alice_pub = [3u8; 32];
        let bob_pub = [4u8; 32];

        let x25519_shared = [5u8; 32];
        let p384_shared = vec![6u8; 48];
        let secp256k1_shared = vec![7u8; 32];
        let frodo_shared = vec![8u8; 24];
        let ntru_shared = vec![9u8; 32];
        let mceliece_shared = vec![10u8; 32];
        let hqc_shared = vec![11u8; 64];
        let p256_shared = vec![12u8; 32];

        let eggs = collect_clutch_eggs(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &alice_pub,
            &bob_pub,
            &x25519_shared,
            &p384_shared,
            &secp256k1_shared,
            &frodo_shared,
            &ntru_shared,
            &mceliece_shared,
            &hqc_shared,
            &p256_shared,
        );

        assert_eq!(eggs.eggs.len(), 14);

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
        let alice_pub = [3u8; 32];
        let bob_pub = [4u8; 32];

        let shared_secret = [99u8; 32];
        let p384_secret = vec![99u8; 48];
        let frodo_secret = vec![99u8; 24];
        let hqc_secret = vec![99u8; 64];

        let eggs = collect_clutch_eggs(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &alice_pub,
            &bob_pub,
            &shared_secret,
            &p384_secret,
            &shared_secret.to_vec(),
            &frodo_secret,
            &shared_secret.to_vec(),
            &shared_secret.to_vec(),
            &hqc_secret,
            &shared_secret.to_vec(),
        );

        let unique_eggs: std::collections::HashSet<Vec<u8>> = eggs.eggs.into_iter().collect();
        assert_eq!(unique_eggs.len(), 14);
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
//    |   <--- ReclutchResponse ---------------   |
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
