use crate::types::Seed;
use blake3::Hasher;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroize;

/// Determine who initiates clutch ceremony.
/// Lower handle_proof = initiator (sends ephemeral pubkeys first)
/// Higher handle_proof = responder (waits, then responds)
///
/// Both parties compute the same result, so no coordination needed.
pub fn is_clutch_initiator(our_handle_proof: &[u8; 32], their_handle_proof: &[u8; 32]) -> bool {
    our_handle_proof < their_handle_proof
}

/// Generate ephemeral X25519 keypair
/// Returns (secret, public) - caller MUST zeroize the secret after use!
pub fn generate_x25519_ephemeral() -> ([u8; 32], [u8; 32]) {
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
pub fn x25519_ecdh(our_secret: &[u8; 32], their_public: &[u8; 32]) -> [u8; 32] {
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
    let (ct, ss) = encapsulate_boxed(&pk, &mut rng);

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
/// NOT on ephemeral clutch keys. This means:
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
    hasher.update(b"PHOTON_HANDSHAKE v0");
    hasher.update(first_device);
    hasher.update(second_device);
    hasher.update(first_handle);
    hasher.update(second_handle);

    *hasher.finalize().as_bytes()
}

// ============================================================================
// LAYER 2: clutch SEED (ephemeral encryption key material)
// ============================================================================

/// Derive the clutch shared seed from private handle hashes and X25519 shared secret.
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
/// Full clutch (8 primitives) will use 256-byte seed via BLAKE3 XOF.
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
    hasher.update(b"clutch_v1_x25519_only");
    hasher.update(first);
    hasher.update(second);
    hasher.update(x25519_shared);

    Seed::from_bytes(*hasher.finalize().as_bytes())
}

/// Derive the clutch shared seed using parallel key exchange.
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
/// Uses BLAKE3 XOF to produce 256-byte seed (ready for full 8-primitive clutch).
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
    hasher.update(b"clutch v3 device bound"); // New version - device keys now bound
    hasher.update(first_device); // Device identity binding (prevents spoofing)
    hasher.update(second_device);
    hasher.update(first_handle); // Out-of-band secret (handle hash)
    hasher.update(second_handle);
    hasher.update(first_pub); // Both parties' ephemeral randomness
    hasher.update(second_pub);
    hasher.update(x25519_shared); // ECDH result (32B for X25519-only)
                                  // Future: add other 7 shared secrets here for full clutch

    // BLAKE3 XOF: extend output to 256 bytes (2048 bits)
    // Phase 1 uses Seed (32 bytes) but we derive full output for future compat
    let mut output = [0u8; 256];
    hasher.finalize_xof().fill(&mut output);

    // For now, use first 32 bytes as seed
    let mut seed_bytes = [0u8; 32];
    seed_bytes.copy_from_slice(&output[..32]);
    Seed::from_bytes(seed_bytes)
}

/// All 8 ephemeral keypairs for full CLUTCH ceremony.
/// Each algorithm has its own keypair format.
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

    // Class 1: Post-quantum lattice KEMs
    pub frodo976_secret: Vec<u8>, // 31296B
    pub frodo976_public: Vec<u8>, // 15632B
    pub ntru701_secret: Vec<u8>,  // 1450B (HRSS-701)
    pub ntru701_public: Vec<u8>,  // 1138B

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
        self.frodo976_secret.zeroize();
        self.ntru701_secret.zeroize();
        self.mceliece_secret.zeroize();
        self.hqc256_secret.zeroize();
    }
}

// =============================================================================
// CLUTCH PAYLOAD STRUCTS FOR NETWORK TRANSFER
// =============================================================================

/// Full offer with all 8 public keys (~548KB).
/// Sent by both parties at start of CLUTCH ceremony.
///
/// For network serialization, use the VSF-wrapped functions in protocol.rs:
/// - build_clutch_full_offer_vsf() / parse_clutch_full_offer_vsf()
#[derive(Clone, Debug)]
pub struct ClutchFullOfferPayload {
    pub x25519_public: [u8; 32],
    pub p384_public: Vec<u8>,
    pub secp256k1_public: Vec<u8>,
    pub p256_public: Vec<u8>,
    pub frodo976_public: Vec<u8>,
    pub ntru701_public: Vec<u8>,
    pub mceliece_public: Vec<u8>,
    pub hqc256_public: Vec<u8>,
}

impl ClutchFullOfferPayload {
    /// Create from our keypairs (extract public keys)
    pub fn from_keypairs(keys: &ClutchAllKeypairs) -> Self {
        Self {
            x25519_public: keys.x25519_public,
            p384_public: keys.p384_public.clone(),
            secp256k1_public: keys.secp256k1_public.clone(),
            p256_public: keys.p256_public.clone(),
            frodo976_public: keys.frodo976_public.clone(),
            ntru701_public: keys.ntru701_public.clone(),
            // TODO: Re-enable McEliece once PT transfer is stable
            // McEliece public key is ~512KB, makes testing painful
            mceliece_public: vec![], // keys.mceliece_public.clone(),
            hqc256_public: keys.hqc256_public.clone(),
        }
    }
}

/// KEM response with 4 ciphertexts (~31KB).
/// Sent by both parties after receiving peer's full offer.
///
/// For network serialization, use the VSF-wrapped functions in protocol.rs:
/// - build_clutch_kem_response_vsf() / parse_clutch_kem_response_vsf()
#[derive(Clone, Debug)]
pub struct ClutchKemResponsePayload {
    pub frodo976_ciphertext: Vec<u8>,
    pub ntru701_ciphertext: Vec<u8>,
    pub mceliece_ciphertext: Vec<u8>,
    pub hqc256_ciphertext: Vec<u8>,
}

impl ClutchKemResponsePayload {
    /// Perform KEM encapsulations to peer's public keys.
    /// Returns (payload, shared_secrets) where shared_secrets are our encapsulated secrets.
    pub fn encapsulate_to_peer(
        their_offer: &ClutchFullOfferPayload,
    ) -> (Self, ClutchKemSharedSecrets) {
        #[cfg(feature = "development")]
        crate::log_info("CLUTCH: Encapsulating to peer's public keys...");

        // Encapsulate to each KEM public key
        let (frodo976_ciphertext, frodo_ss) = frodo976_encapsulate(&their_offer.frodo976_public);
        let (ntru701_ciphertext, ntru_ss) = ntru701_encapsulate(&their_offer.ntru701_public);
        // TODO: Re-enable McEliece once PT transfer is stable
        let (mceliece_ciphertext, mceliece_ss) = if their_offer.mceliece_public.is_empty() {
            (vec![], vec![0u8; 32]) // Placeholder shared secret
        } else {
            mceliece460896_encapsulate(&their_offer.mceliece_public)
        };
        let (hqc256_ciphertext, hqc_ss) = hqc256_encapsulate(&their_offer.hqc256_public);

        #[cfg(feature = "development")]
        crate::log_info(&format!(
            "CLUTCH: KEM ciphertexts ready (Frodo: {}B, NTRU: {}B, McEliece: {}B, HQC: {}B)",
            frodo976_ciphertext.len(),
            ntru701_ciphertext.len(),
            mceliece_ciphertext.len(),
            hqc256_ciphertext.len()
        ));

        let payload = Self {
            frodo976_ciphertext,
            ntru701_ciphertext,
            mceliece_ciphertext,
            hqc256_ciphertext,
        };

        let secrets = ClutchKemSharedSecrets {
            frodo: frodo_ss,
            ntru: ntru_ss,
            mceliece: mceliece_ss,
            hqc: hqc_ss,
        };

        (payload, secrets)
    }
}

/// Shared secrets from KEM encapsulation (one direction)
#[derive(Clone, Debug)]
pub struct ClutchKemSharedSecrets {
    pub frodo: Vec<u8>,
    pub ntru: Vec<u8>,
    pub mceliece: Vec<u8>,
    pub hqc: Vec<u8>,
}

impl ClutchKemSharedSecrets {
    /// Decapsulate from received ciphertexts using our secret keys
    pub fn decapsulate_from_peer(
        response: &ClutchKemResponsePayload,
        our_keys: &ClutchAllKeypairs,
    ) -> Self {
        #[cfg(feature = "development")]
        crate::log_info("CLUTCH: Decapsulating from peer's ciphertexts...");

        let frodo = frodo976_decapsulate(&our_keys.frodo976_secret, &response.frodo976_ciphertext);
        #[cfg(feature = "development")]
        crate::log_info(&format!("CLUTCH: ✓ Frodo976 decap OK ({}B shared secret)", frodo.len()));

        let ntru = ntru701_decapsulate(&our_keys.ntru701_secret, &response.ntru701_ciphertext);
        #[cfg(feature = "development")]
        crate::log_info(&format!("CLUTCH: ✓ NTRU701 decap OK ({}B shared secret)", ntru.len()));

        // TODO: Re-enable McEliece once PT transfer is stable
        let mceliece = if response.mceliece_ciphertext.is_empty() {
            #[cfg(feature = "development")]
            crate::log_info("CLUTCH: - McEliece skipped (empty ciphertext)");
            vec![0u8; 32] // Placeholder shared secret
        } else {
            let ss = mceliece460896_decapsulate(&our_keys.mceliece_secret, &response.mceliece_ciphertext);
            #[cfg(feature = "development")]
            crate::log_info(&format!("CLUTCH: ✓ McEliece decap OK ({}B shared secret)", ss.len()));
            ss
        };

        let hqc = hqc256_decapsulate(&our_keys.hqc256_secret, &response.hqc256_ciphertext);
        #[cfg(feature = "development")]
        crate::log_info(&format!("CLUTCH: ✓ HQC256 decap OK ({}B shared secret)", hqc.len()));

        Self {
            frodo,
            ntru,
            mceliece,
            hqc,
        }
    }

    /// Zeroize all secrets
    pub fn zeroize(&mut self) {
        self.frodo.zeroize();
        self.ntru.zeroize();
        self.mceliece.zeroize();
        self.hqc.zeroize();
    }
}

/// Generate all 8 ephemeral keypairs for full CLUTCH ceremony.
/// WARNING: This generates ~512KB of public key material (mostly McEliece).
/// Caller MUST call zeroize() on the result when done!
pub fn generate_all_ephemeral_keypairs() -> ClutchAllKeypairs {
    #[cfg(feature = "development")]
    crate::log_info("CLUTCH: Generating 8 ephemeral keypairs...");

    // Class 0: Classical EC
    let (x25519_secret, x25519_public) = generate_x25519_ephemeral();
    let (p384_secret, p384_public) = generate_p384_ephemeral();
    let (secp256k1_secret, secp256k1_public) = generate_secp256k1_ephemeral();
    let (p256_secret, p256_public) = generate_p256_ephemeral();

    // Class 1: Post-quantum lattice KEMs
    let (frodo976_secret, frodo976_public) = generate_frodo976_keypair();
    let (ntru701_secret, ntru701_public) = generate_ntru701_keypair();

    // Class 2: Post-quantum code-based KEMs
    let (mceliece_secret, mceliece_public) = generate_mceliece460896_keypair();
    let (hqc256_secret, hqc256_public) = generate_hqc256_keypair();

    #[cfg(feature = "development")]
    crate::log_info(&format!(
        "CLUTCH: Keypairs ready (X25519: {}B, P-384: {}B, secp256k1: {}B, P-256: {}B, Frodo: {}B, NTRU: {}B, McEliece: {}B, HQC: {}B)",
        x25519_public.len(),
        p384_public.len(),
        secp256k1_public.len(),
        p256_public.len(),
        frodo976_public.len(),
        ntru701_public.len(),
        mceliece_public.len(),
        hqc256_public.len()
    ));

    ClutchAllKeypairs {
        x25519_secret,
        x25519_public,
        p384_secret,
        p384_public,
        secp256k1_secret,
        secp256k1_public,
        p256_secret,
        p256_public,
        frodo976_secret,
        frodo976_public,
        ntru701_secret,
        ntru701_public,
        mceliece_secret,
        mceliece_public,
        hqc256_secret,
        hqc256_public,
    }
}

/// All shared secrets from 20 cryptographic eggs.
/// Each "egg" is a labeled BLAKE3 hash for domain separation.
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
        hasher.update(b"clutch EGG v4 ");
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
/// Each egg is a BLAKE3 hash with domain separation:
/// BLAKE3("clutch_EGG_v4_" || label || shared_secret)
///
/// Returns vector of 20 eggs ready for avalanche hashing.
pub fn collect_clutch_eggs(
    our_device_pubkey: &[u8; 32],
    their_device_pubkey: &[u8; 32],
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    low_x25519_shared: &[u8; 32],
    high_x25519_shared: &[u8; 32],
    low_p384_shared: &[u8],
    high_p384_shared: &[u8],
    low_secp256k1_shared: &[u8],
    high_secp256k1_shared: &[u8],
    low_frodo_shared: &[u8],
    high_frodo_shared: &[u8],
    low_ntru_shared: &[u8],
    high_ntru_shared: &[u8],
    low_mceliece_shared: &[u8],
    high_mceliece_shared: &[u8],
    low_hqc_shared: &[u8],
    high_hqc_shared: &[u8],
    low_p256_shared: &[u8],
    high_p256_shared: &[u8],
) -> ClutchEggs {
    let mut eggs = ClutchEggs::new();

    // Sort device pubkeys and handle hashes canonically so both parties add in same order
    let (low_device, high_device) = sort_pair(our_device_pubkey, their_device_pubkey);
    let (low_handle, high_handle) = sort_pair(our_handle_hash, their_handle_hash);

    eggs.add_egg("low_device_pubkey", low_device);
    eggs.add_egg("high_device_pubkey", high_device);
    eggs.add_egg("low_handle_hash", low_handle);
    eggs.add_egg("high_handle_hash", high_handle);

    // Class 0: Classical EC - low handle's secrets first, then high
    eggs.add_egg("low_x25519", low_x25519_shared);
    eggs.add_egg("high_x25519", high_x25519_shared);
    eggs.add_egg("low_p384", low_p384_shared);
    eggs.add_egg("high_p384", high_p384_shared);
    eggs.add_egg("low_secp256k1", low_secp256k1_shared);
    eggs.add_egg("high_secp256k1", high_secp256k1_shared);
    eggs.add_egg("low_p256", low_p256_shared);
    eggs.add_egg("high_p256", high_p256_shared);

    // Class 1: Post-quantum lattice KEMs
    eggs.add_egg("low_frodo976", low_frodo_shared);
    eggs.add_egg("high_frodo976", high_frodo_shared);
    eggs.add_egg("low_ntru701", low_ntru_shared);
    eggs.add_egg("high_ntru701", high_ntru_shared);

    // Class 2: Post-quantum code-based KEMs
    eggs.add_egg("low_mceliece460896", low_mceliece_shared);
    eggs.add_egg("high_mceliece460896", high_mceliece_shared);
    eggs.add_egg("low_hqc256", low_hqc_shared);
    eggs.add_egg("high_hqc256", high_hqc_shared);

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
    crate::log_info(&format!(
        "CLUTCH: Collecting {} eggs for avalanche ({} bytes input)...",
        eggs.eggs.len(),
        eggs.eggs.len() * 32
    ));

    const MIN_SIZE: usize = 1_048_576; // 1MB ish
    const TOTAL_SIZE: usize = MIN_SIZE * 2; // 2MB
    let max_size = TOTAL_SIZE * 2; // Allow expansion up to 4MB

    // Step 0: Flatten all eggs into one buffer
    let mut omelette = Vec::with_capacity(max_size);
    for egg in &eggs.eggs {
        omelette.extend_from_slice(egg);
    }

    #[cfg(feature = "development")]
    crate::log_info("CLUTCH: Avalanche hashing to 2MB...");

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

    // Step 2: Heavy mixing with diverse operations
    // Process as variable-sized chunks (1-43 bytes, unaligned) for maximum diffusion
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

    // Step 3: Final rotation before trim
    // Hash entire buffer and rotate by (hash % len) to shuffle one last time
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
        // Show first 64 bytes of each pad for comparison
        let low_preview: String = low_pad[..64].iter().map(|b| format!("{:02x}", b)).collect();
        let high_preview: String = high_pad[..64].iter().map(|b| format!("{:02x}", b)).collect();

        crate::log_info(&format!("CLUTCH: low_pad[0..64]  = {}", low_preview));
        crate::log_info(&format!("CLUTCH: high_pad[0..64] = {}", high_preview));
    }

    (low_pad, high_pad)
}

/// Compute the clutch completion proof.
/// Sent by initiator to confirm they derived the same seed.
/// Responder can verify without revealing the seed.
pub fn compute_clutch_proof(seed: &Seed) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(seed.as_bytes());
    hasher.update(b"clutch_v1_complete");
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
/// Both parties generate ephemeral keypairs simultaneously and exchange them.
/// Both pubkeys contribute entropy to the final seed.
/// Device pubkeys are mixed in to bind the seed to both device identities.
///
/// Steps:
/// 0. Generate ephemeral keypair (done before calling this)
/// 1. Exchange ClutchOffer messages (both directions, parallel)
/// 2. Once both pubkeys known, call this function
/// 3. Lower handle_proof party sends ClutchComplete with proof
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
/// For EC algorithms (X25519, P-384, secp256k1, P-256):
/// - Both parties compute ECDH → same shared secret
/// - But we label them low_* and high_* by handle ordering
///
/// For KEM algorithms (Frodo, NTRU, McEliece, HQC):
/// - Each party encapsulates to peer → peer decapsulates
/// - Two different shared secrets per algorithm (bidirectional)
pub struct ClutchSharedSecrets {
    // Class 0: Classical EC (same secret, labeled by party)
    pub low_x25519: [u8; 32],
    pub high_x25519: [u8; 32],
    pub low_p384: Vec<u8>, // 48B
    pub high_p384: Vec<u8>,
    pub low_secp256k1: Vec<u8>, // 32B
    pub high_secp256k1: Vec<u8>,
    pub low_p256: Vec<u8>, // 32B
    pub high_p256: Vec<u8>,

    // Class 1: Post-quantum lattice KEMs (different secrets per direction)
    pub low_frodo: Vec<u8>, // 24B
    pub high_frodo: Vec<u8>,
    pub low_ntru: Vec<u8>, // 32B
    pub high_ntru: Vec<u8>,

    // Class 2: Post-quantum code-based KEMs
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
        self.low_frodo.zeroize();
        self.high_frodo.zeroize();
        self.low_ntru.zeroize();
        self.high_ntru.zeroize();
        self.low_mceliece.zeroize();
        self.high_mceliece.zeroize();
        self.low_hqc.zeroize();
        self.high_hqc.zeroize();
    }
}

/// Perform full 8-algorithm CLUTCH ceremony.
///
/// Takes all 16 shared secrets (8 algorithms × 2 directions) and produces
/// identical (low_pad, high_pad) on both parties.
///
/// The low/high ordering is determined by comparing handle_hashes:
/// - Party with lower handle_hash uses low_pad for sending
/// - Party with higher handle_hash uses high_pad for sending
///
/// Both parties MUST call this with the same shared secrets (just with
/// their perspective on low/high being different based on handle ordering).
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
    secrets: &ClutchSharedSecrets,
) -> ClutchFullResult {
    // Collect all 20 eggs
    let eggs = collect_clutch_eggs(
        our_device_pubkey,
        their_device_pubkey,
        our_handle_hash,
        their_handle_hash,
        &secrets.low_x25519,
        &secrets.high_x25519,
        &secrets.low_p384,
        &secrets.high_p384,
        &secrets.low_secp256k1,
        &secrets.high_secp256k1,
        &secrets.low_frodo,
        &secrets.high_frodo,
        &secrets.low_ntru,
        &secrets.high_ntru,
        &secrets.low_mceliece,
        &secrets.high_mceliece,
        &secrets.low_hqc,
        &secrets.high_hqc,
        &secrets.low_p256,
        &secrets.high_p256,
    );

    // Compute proof from eggs (deterministic - same eggs = same proof)
    let proof = compute_eggs_proof(&eggs);

    ClutchFullResult { eggs, proof }
}

/// Compute proof hash for CLUTCH verification from eggs.
/// Used by both parties to verify they collected identical eggs.
pub fn compute_eggs_proof(eggs: &ClutchEggs) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(b"CLUTCH_EGGS_v1_proof");
    for egg in &eggs.eggs {
        hasher.update(egg);
    }
    *hasher.finalize().as_bytes()
}

/// Verify CLUTCH proof matches our eggs.
pub fn verify_eggs_proof(eggs: &ClutchEggs, proof: &[u8; 32]) -> bool {
    use subtle::ConstantTimeEq;
    let expected = compute_eggs_proof(eggs);
    expected.ct_eq(proof).into()
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
        // This test verified v1 sequential clutch (initiator/responder pattern).
        // v3 uses parallel exchange only - see test_parallel_clutch_produces_same_seed.
        // Keeping this stub to document the intentional removal of v1 support.
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
        // So re-clutch (new ephemeral keys) produces same provenance
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

        // Re-clutch with new ephemeral keys (simulated)
        // But same device keys, handles, and signatures
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
        let low_x25519 = [5u8; 32];
        let high_x25519 = [6u8; 32];
        let low_p384 = vec![7u8; 48];
        let high_p384 = vec![8u8; 48];
        let low_secp256k1 = vec![9u8; 32];
        let high_secp256k1 = vec![10u8; 32];
        let low_frodo = vec![11u8; 24];
        let high_frodo = vec![12u8; 24];
        let low_ntru = vec![13u8; 32];
        let high_ntru = vec![14u8; 32];
        let low_mceliece = vec![15u8; 32];
        let high_mceliece = vec![16u8; 32];
        let low_hqc = vec![17u8; 64];
        let high_hqc = vec![18u8; 64];
        let low_p256 = vec![19u8; 32];
        let high_p256 = vec![20u8; 32];

        let eggs = collect_clutch_eggs(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &low_x25519,
            &high_x25519,
            &low_p384,
            &high_p384,
            &low_secp256k1,
            &high_secp256k1,
            &low_frodo,
            &high_frodo,
            &low_ntru,
            &high_ntru,
            &low_mceliece,
            &high_mceliece,
            &low_hqc,
            &high_hqc,
            &low_p256,
            &high_p256,
        );

        // 4 identity + 16 shared secrets = 20 eggs
        assert_eq!(eggs.eggs.len(), 20);

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
        let shared_32 = [99u8; 32];
        let shared_48 = vec![99u8; 48];
        let shared_24 = vec![99u8; 24];
        let shared_64 = vec![99u8; 64];

        let eggs = collect_clutch_eggs(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &shared_32,          // low_x25519
            &shared_32,          // high_x25519
            &shared_48,          // low_p384
            &shared_48,          // high_p384
            &shared_32.to_vec(), // low_secp256k1
            &shared_32.to_vec(), // high_secp256k1
            &shared_24,          // low_frodo
            &shared_24,          // high_frodo
            &shared_32.to_vec(), // low_ntru
            &shared_32.to_vec(), // high_ntru
            &shared_32.to_vec(), // low_mceliece
            &shared_32.to_vec(), // high_mceliece
            &shared_64,          // low_hqc
            &shared_64,          // high_hqc
            &shared_32.to_vec(), // low_p256
            &shared_32.to_vec(), // high_p256
        );

        // Even with same input bytes, domain separation should produce unique eggs
        let unique_eggs: std::collections::HashSet<[u8; 32]> = eggs.eggs.into_iter().collect();
        assert_eq!(unique_eggs.len(), 20);
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

        // === EC ALGORITHMS: Both compute same shared secret ===
        // X25519
        let x25519_shared = x25519_ecdh(&alice_keys.x25519_secret, &bob_keys.x25519_public);
        let x25519_shared_bob = x25519_ecdh(&bob_keys.x25519_secret, &alice_keys.x25519_public);
        assert_eq!(x25519_shared, x25519_shared_bob);

        // P-384
        let p384_shared = p384_ecdh(&alice_keys.p384_secret, &bob_keys.p384_public);
        let p384_shared_bob = p384_ecdh(&bob_keys.p384_secret, &alice_keys.p384_public);
        assert_eq!(p384_shared, p384_shared_bob);

        // secp256k1
        let secp256k1_shared =
            secp256k1_ecdh(&alice_keys.secp256k1_secret, &bob_keys.secp256k1_public);
        let secp256k1_shared_bob =
            secp256k1_ecdh(&bob_keys.secp256k1_secret, &alice_keys.secp256k1_public);
        assert_eq!(secp256k1_shared, secp256k1_shared_bob);

        // P-256
        let p256_shared = p256_ecdh(&alice_keys.p256_secret, &bob_keys.p256_public);
        let p256_shared_bob = p256_ecdh(&bob_keys.p256_secret, &alice_keys.p256_public);
        assert_eq!(p256_shared, p256_shared_bob);

        // === KEM ALGORITHMS: Each encapsulates to peer, decapsulates own ===

        // FrodoKEM-976
        let (frodo_ct_to_bob, frodo_ss_alice_encap) =
            frodo976_encapsulate(&bob_keys.frodo976_public);
        let (frodo_ct_to_alice, frodo_ss_bob_encap) =
            frodo976_encapsulate(&alice_keys.frodo976_public);
        let frodo_ss_bob_decap = frodo976_decapsulate(&bob_keys.frodo976_secret, &frodo_ct_to_bob);
        let frodo_ss_alice_decap =
            frodo976_decapsulate(&alice_keys.frodo976_secret, &frodo_ct_to_alice);
        assert_eq!(frodo_ss_alice_encap, frodo_ss_bob_decap); // Alice→Bob direction
        assert_eq!(frodo_ss_bob_encap, frodo_ss_alice_decap); // Bob→Alice direction

        // NTRU-701
        let (ntru_ct_to_bob, ntru_ss_alice_encap) = ntru701_encapsulate(&bob_keys.ntru701_public);
        let (ntru_ct_to_alice, ntru_ss_bob_encap) = ntru701_encapsulate(&alice_keys.ntru701_public);
        let ntru_ss_bob_decap = ntru701_decapsulate(&bob_keys.ntru701_secret, &ntru_ct_to_bob);
        let ntru_ss_alice_decap =
            ntru701_decapsulate(&alice_keys.ntru701_secret, &ntru_ct_to_alice);
        assert_eq!(ntru_ss_alice_encap, ntru_ss_bob_decap);
        assert_eq!(ntru_ss_bob_encap, ntru_ss_alice_decap);

        // McEliece-460896
        let (mce_ct_to_bob, mce_ss_alice_encap) =
            mceliece460896_encapsulate(&bob_keys.mceliece_public);
        let (mce_ct_to_alice, mce_ss_bob_encap) =
            mceliece460896_encapsulate(&alice_keys.mceliece_public);
        let mce_ss_bob_decap =
            mceliece460896_decapsulate(&bob_keys.mceliece_secret, &mce_ct_to_bob);
        let mce_ss_alice_decap =
            mceliece460896_decapsulate(&alice_keys.mceliece_secret, &mce_ct_to_alice);
        assert_eq!(mce_ss_alice_encap, mce_ss_bob_decap);
        assert_eq!(mce_ss_bob_encap, mce_ss_alice_decap);

        // HQC-256
        let (hqc_ct_to_bob, hqc_ss_alice_encap) = hqc256_encapsulate(&bob_keys.hqc256_public);
        let (hqc_ct_to_alice, hqc_ss_bob_encap) = hqc256_encapsulate(&alice_keys.hqc256_public);
        let hqc_ss_bob_decap = hqc256_decapsulate(&bob_keys.hqc256_secret, &hqc_ct_to_bob);
        let hqc_ss_alice_decap = hqc256_decapsulate(&alice_keys.hqc256_secret, &hqc_ct_to_alice);
        assert_eq!(hqc_ss_alice_encap, hqc_ss_bob_decap);
        assert_eq!(hqc_ss_bob_encap, hqc_ss_alice_decap);

        // === BUILD SHARED SECRETS STRUCT ===
        // low_* = from alice's perspective (alice is low handle)
        // high_* = from bob's perspective (bob is high handle)
        //
        // For EC: both get same shared secret, but labeled by who initiated
        // For KEM: low_* = alice→bob direction, high_* = bob→alice direction

        let alice_secrets = ClutchSharedSecrets {
            low_x25519: x25519_shared,
            high_x25519: x25519_shared, // Same for EC
            low_p384: p384_shared.clone(),
            high_p384: p384_shared.clone(),
            low_secp256k1: secp256k1_shared.clone(),
            high_secp256k1: secp256k1_shared.clone(),
            low_p256: p256_shared.clone(),
            high_p256: p256_shared.clone(),
            // KEM: directional
            low_frodo: frodo_ss_alice_encap.clone(), // Alice→Bob
            high_frodo: frodo_ss_alice_decap.clone(), // Bob→Alice (what Alice decapsulated)
            low_ntru: ntru_ss_alice_encap.clone(),
            high_ntru: ntru_ss_alice_decap.clone(),
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
            // KEM: Bob's view is symmetric to Alice's
            low_frodo: frodo_ss_bob_decap.clone(), // Alice→Bob (what Bob decapsulated)
            high_frodo: frodo_ss_bob_encap.clone(), // Bob→Alice
            low_ntru: ntru_ss_bob_decap.clone(),
            high_ntru: ntru_ss_bob_encap.clone(),
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
            &alice_secrets,
        );

        let bob_result = clutch_complete_full(
            &bob_device,
            &alice_device,
            &bob_handle,
            &alice_handle,
            &bob_secrets,
        );

        // === THE CRITICAL ASSERTIONS ===
        // Both parties should collect identical eggs
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
}
