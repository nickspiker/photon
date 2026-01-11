//! Rolling chain encryption for messages (CHAIN protocol).
//!
//! 512-link Chain (16KB):
//! - Links [0..256) = history (zeros initially, fills as chain advances)
//! - Links [256..512) = active (derived from CLUTCH, current key at [511])
//! - Per-participant chains in friendships (N chains for N-party)
//!
//! On advance: left-shift all links, derive new link at [511] via spaghettify.
//! This provides forward secrecy - past keys are destroyed.

use super::clutch::{smear_hash, spaghettify};
use chacha20::{
    cipher::{KeyIvInit, StreamCipher},
    ChaCha20,
};
use thiserror::Error;
use vsf::EagleTime;

#[derive(Debug, Error)]
pub enum ChainError {
    #[error("Decryption failed")]
    DecryptionFailed,

    #[error("Invalid message format")]
    InvalidMessage,

    #[error("Chain not initialized")]
    NotInitialized,

    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),

    #[error("Invalid VSF signature")]
    SignatureInvalid,

    #[error("Invalid Photon encoding (expected 'P')")]
    InvalidEncoding,

    #[error("ACK proof mismatch")]
    AckMismatch,

    #[error("Participant not found in friendship")]
    ParticipantNotFound,
}

pub type Result<T> = std::result::Result<T, ChainError>;

// ============================================================================
// Constants from CHAIN.md Appendix A
// ============================================================================

/// Total links in a chain (512 × 32B = 16KB)
pub const CHAIN_LINKS: usize = 512;

/// History links [0..256)
pub const HISTORY_LINKS: usize = 256;

/// Active links [256..512)
pub const ACTIVE_LINKS: usize = 256;

/// Size of each link in bytes
pub const LINK_SIZE: usize = 32;

/// Total chain size in bytes (16KB)
pub const CHAIN_SIZE: usize = CHAIN_LINKS * LINK_SIZE;

/// Current key position (rightmost = newest)
pub const CURRENT_KEY_INDEX: usize = 511;

/// L1 scratch pad size (30KB - fits in L1 cache)
pub const L1_SIZE: usize = 30_720;

/// L1 mixing rounds
pub const L1_ROUNDS: usize = 3;

// Domain separation strings
const DOMAIN_ADVANCE: &[u8] = b"PHOTON_ADVANCE_v0";
const DOMAIN_ACK: &[u8] = b"PHOTON_ACK_v0";
const DOMAIN_CONFIRM: &[u8] = b"PHOTON_CONFIRM_v0";
const DOMAIN_SALT: &[u8] = b"PHOTON_SALT_v0";

// Link ranges for different operations
const ACK_LINK_RANGE: std::ops::Range<usize> = 507..512; // 5 links (160B)
const CONFIRM_LINK_RANGE: std::ops::Range<usize> = 509..512; // 3 links (96B)
const SALT_LINK_RANGE: std::ops::Range<usize> = 500..512; // 12 links (384B)

// ============================================================================
// Chain Structure
// ============================================================================

/// 512-link chain (16KB) - one per participant in a friendship
///
/// Layout:
/// - links[0..256): History window (zeros initially, fills as chain advances)
/// - links[256..512): Active chain (derived from CLUTCH)
/// - links[511]: Current encryption key (rightmost = newest)
///
/// On advance: left-shift all links, old [256] becomes [255] (newest history),
/// new link derived at [511] via spaghettify.
#[derive(Clone)]
pub struct Chain {
    /// 512 links × 32 bytes = 16KB
    links: [[u8; 32]; CHAIN_LINKS],

    /// Last ACKed Eagle time for this participant
    pub last_ack_time: Option<EagleTime>,
}

impl Chain {
    /// Create a new chain from raw bytes (8KB active portion from CLUTCH avalanche).
    ///
    /// The 8KB goes into links[256..512] (active portion).
    /// links[0..256] (history) is initialized to zeros.
    ///
    /// Returns None if bytes.len() != 8192
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != ACTIVE_LINKS * LINK_SIZE {
            return None;
        }

        let mut links = [[0u8; 32]; CHAIN_LINKS];

        // History [0..256) stays zeros
        // Active [256..512) gets the CLUTCH-derived bytes
        for (i, chunk) in bytes.chunks_exact(LINK_SIZE).enumerate() {
            links[HISTORY_LINKS + i].copy_from_slice(chunk);
        }

        Some(Self {
            links,
            last_ack_time: None,
        })
    }

    /// Serialize chain to raw bytes (16KB - full chain for storage/sync)
    pub fn to_bytes(&self) -> Vec<u8> {
        self.links.as_flattened().to_vec()
    }

    /// Restore chain from full 16KB storage
    pub fn from_full_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != CHAIN_SIZE {
            return None;
        }

        let mut links = [[0u8; 32]; CHAIN_LINKS];
        for (i, chunk) in bytes.chunks_exact(LINK_SIZE).enumerate() {
            links[i].copy_from_slice(chunk);
        }

        Some(Self {
            links,
            last_ack_time: None,
        })
    }

    /// Get the current encryption key (always link[511] - rightmost = newest)
    #[inline]
    pub fn current_key(&self) -> &[u8; 32] {
        &self.links[CURRENT_KEY_INDEX]
    }

    /// Get a reference to link at index
    pub fn link(&self, index: usize) -> Option<&[u8; 32]> {
        self.links.get(index)
    }

    /// Get links as a slice (for hashing operations)
    pub fn links(&self) -> &[[u8; 32]; CHAIN_LINKS] {
        &self.links
    }

    /// Advance the chain after ACK confirmation.
    ///
    /// Algorithm (from CHAIN.md Section 7.1):
    /// 1. Left-shift all links (oldest history at [0] drops off)
    /// 2. Old [256] (oldest active) becomes [255] (newest history)
    /// 3. Derive new link at [511] via spaghettify
    ///
    /// Bidirectional entropy: if `their_plaintext` is provided, it's mixed into
    /// the fresh link derivation. This means the other party's message content
    /// contributes entropy to our chain advancement.
    pub fn advance(
        &mut self,
        eagle_time: &EagleTime,
        our_plaintext: &[u8],
        their_plaintext: Option<&[u8]>,
    ) {
        // Left-shift: everything moves left, oldest drops off [0]
        self.links.copy_within(1..CHAIN_LINKS, 0);

        // Derive fresh link via spaghettify (computationally chaotic)
        // With bidirectional entropy mixing if their_plaintext is provided
        let fresh_link =
            derive_fresh_link(&eagle_time, our_plaintext, their_plaintext, &self.links);

        // Append at rightmost position
        self.links[CURRENT_KEY_INDEX] = fresh_link;

        // Update last ack time
        self.last_ack_time = Some(eagle_time.clone());
    }
}

impl std::fmt::Debug for Chain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Don't leak key material - just show structure
        write!(f, "Chain {{ {} links, 16KB }}", CHAIN_LINKS)
    }
}

// ============================================================================
// Salt Derivation (Section 3.0)
// ============================================================================

/// Derive salt from previous plaintext and chain state.
///
/// Each message's salt is derived from the previous message's plaintext.
/// This creates a cryptographic chain that forces message ordering.
///
/// For first message: pass empty slice as prev_plaintext.
pub fn derive_salt(prev_plaintext: &[u8], chain: &Chain) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DOMAIN_SALT);
    hasher.update(prev_plaintext); // Empty for first message
    hasher.update(chain.links[SALT_LINK_RANGE].as_flattened()); // Last 12 links (384B)
    spaghettify(hasher.finalize().as_bytes())
}

// ============================================================================
// Scratch Pad Generation (Section 4.0)
// ============================================================================

/// Generate L1 scratch pad for XOR layer.
///
/// Memory-hard, data-dependent mixing that fits in L1 cache.
/// Uses smear_hash for algorithm diversity.
pub fn generate_scratch(chain: &Chain, salt: &[u8; 32]) -> Vec<u8> {
    let mut scratch = vec![0u8; L1_SIZE];

    // Initialize from current key (link[511]) XOR salt
    let mut state = [0u8; 32];
    for i in 0..32 {
        state[i] = chain.links[CURRENT_KEY_INDEX][i] ^ salt[i];
    }
    scratch[0..32].copy_from_slice(&state);

    // Fill with sequential hashing
    for i in (32..L1_SIZE).step_by(32) {
        state = smear_hash(&scratch[i - 32..i]);
        scratch[i..i + 32].copy_from_slice(&state);
    }

    // Data-dependent mixing rounds
    for _round in 0..L1_ROUNDS {
        for i in (32..L1_SIZE).step_by(32) {
            // Read position depends on current state
            let read_idx = (u32::from_le_bytes(scratch[i..i + 4].try_into().unwrap()) as usize)
                % (i / 32)
                * 32;

            // Mix with data-dependent read
            let mut mix_input = [0u8; 64];
            mix_input[0..32].copy_from_slice(&scratch[i - 32..i]);
            mix_input[32..64].copy_from_slice(&scratch[read_idx..read_idx + 32]);

            let mixed = smear_hash(&mix_input);
            scratch[i..i + 32].copy_from_slice(&mixed);
        }
    }

    scratch
}

// ============================================================================
// Confirmation Smear (Inner Integrity - Section 5.1)
// ============================================================================

/// Generate confirmation smear for inner integrity.
///
/// Chain-bound, encrypted inside the message. Proves possession of chain state.
/// Uses last 3 links (96B) for binding.
pub fn generate_confirmation_smear(message: &[u8], chain: &Chain) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DOMAIN_CONFIRM);
    hasher.update(message);
    hasher.update(chain.links[CONFIRM_LINK_RANGE].as_flattened()); // Last 3 links (96B)
    smear_hash(hasher.finalize().as_bytes())
}

// ============================================================================
// ACK Proof (Section 6.1)
// ============================================================================

/// Generate ACK proof for message acknowledgment.
///
/// Fast (smear_hash, not spaghettify) - keeps message flow snappy.
/// Domain-separated from fresh_link derivation.
pub fn generate_ack_proof(
    eagle_time: &EagleTime,
    plaintext_hash: &[u8; 32],
    chain: &Chain,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DOMAIN_ACK);
    hasher.update(plaintext_hash); // Hash first (opposite order from advance)
    hasher.update(&eagle_time.to_f64().to_le_bytes());
    hasher.update(chain.links[ACK_LINK_RANGE].as_flattened()); // Last 5 links (160B)

    smear_hash(hasher.finalize().as_bytes())
}

/// Verify ACK proof
pub fn verify_ack_proof(
    eagle_time: &EagleTime,
    plaintext_hash: &[u8; 32],
    chain: &Chain,
    received_proof: &[u8; 32],
) -> bool {
    let expected = generate_ack_proof(eagle_time, plaintext_hash, chain);
    constant_time_eq(&expected, received_proof)
}

// ============================================================================
// Chain Advancement (Section 7.1)
// ============================================================================

/// Derive fresh link for chain advancement.
///
/// Uses spaghettify for computational chaos (data-dependent ops, IEEE754 weirdness).
/// NOT memory-hard (~1.7KB state) - fast enough for per-message use.
///
/// Bidirectional entropy: both plaintexts (ours and theirs) are fed directly to
/// spaghettify. No pre-hashing - spaghettify gets all the raw entropy.
/// Domain separation via clear structure: domain + lengths + data
fn derive_fresh_link(
    eagle_time: &EagleTime,
    our_plaintext: &[u8],
    their_plaintext: Option<&[u8]>,
    chain: &[[u8; 32]; CHAIN_LINKS],
) -> [u8; 32] {
    // Active chain portion (post-shift, so [256..511] now, [255..510] after shift)
    let chain_portion = chain[HISTORY_LINKS..CURRENT_KEY_INDEX].as_flattened();

    // Build input with clear domain separation:
    // domain_tag + eagle_time + our_len + our_plaintext + chain_portion + [their_len + their_plaintext]
    let mut input = Vec::with_capacity(
        DOMAIN_ADVANCE.len()
            + 8
            + 4
            + our_plaintext.len()
            + chain_portion.len()
            + 4
            + their_plaintext.map_or(0, |p| p.len()),
    );

    input.extend_from_slice(DOMAIN_ADVANCE);
    input.extend_from_slice(&eagle_time.to_f64().to_le_bytes());
    input.extend_from_slice(&(our_plaintext.len() as u32).to_le_bytes());
    input.extend_from_slice(our_plaintext);
    input.extend_from_slice(chain_portion);

    // Add their plaintext if available (bidirectional weave)
    if let Some(their_pt) = their_plaintext {
        input.extend_from_slice(&(their_pt.len() as u32).to_le_bytes());
        input.extend_from_slice(their_pt);
    }

    // Feed raw bytes directly to spaghettify - no pre-hash bottleneck
    spaghettify(&input)
}

// ============================================================================
// ChaCha20 Nonce Derivation
// ============================================================================

/// Derive ChaCha20 nonce from Eagle time.
///
/// Uses first 12 bytes of BLAKE3 hash of timestamp.
pub fn derive_nonce(eagle_time: &EagleTime) -> [u8; 12] {
    let hash = blake3::hash(&eagle_time.to_f64().to_le_bytes());
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&hash.as_bytes()[..12]);
    nonce
}

// ============================================================================
// 3-Layer Encryption (Section 5.1)
// ============================================================================

/// Encrypt plaintext using 3-layer encryption.
///
/// Layer 1: Build VSF section (done by caller)
/// Layer 2: ChaCha20 encryption
/// Layer 3: XOR with scratch pad
pub fn encrypt_layers(
    plaintext: &[u8],
    chain: &Chain,
    scratch: &[u8],
    eagle_time: &EagleTime,
) -> Vec<u8> {
    // Derive ChaCha20 key from current link (rightmost = newest)
    let chacha_key = blake3::derive_key("photon.chain.chacha.v0", chain.current_key());

    // Derive nonce from eagle time
    let nonce = derive_nonce(eagle_time);

    // Layer 2: ChaCha20 encryption
    let mut cipher = ChaCha20::new(&chacha_key.into(), &nonce.into());
    let mut ciphertext = plaintext.to_vec();
    cipher.apply_keystream(&mut ciphertext);

    // Layer 3: XOR with scratch pad (cycling if message > scratch)
    for (i, byte) in ciphertext.iter_mut().enumerate() {
        *byte ^= scratch[i % scratch.len()];
    }

    ciphertext
}

/// Decrypt ciphertext using 3-layer decryption.
///
/// Layer 3: XOR with scratch pad
/// Layer 2: ChaCha20 decryption
/// Layer 1: Parse VSF section (done by caller)
pub fn decrypt_layers(
    ciphertext: &[u8],
    chain: &Chain,
    key_index: usize,
    scratch: &[u8],
    eagle_time: &EagleTime,
) -> Vec<u8> {
    // Layer 3 (reverse): XOR with scratch pad
    let mut intermediate = ciphertext.to_vec();
    for (i, byte) in intermediate.iter_mut().enumerate() {
        *byte ^= scratch[i % scratch.len()];
    }

    // Derive ChaCha20 key from specified link
    let chacha_key = blake3::derive_key("photon.chain.chacha.v0", &chain.links[key_index]);

    // Derive nonce from eagle time
    let nonce = derive_nonce(eagle_time);

    // Layer 2 (reverse): ChaCha20 decryption
    let mut cipher = ChaCha20::new(&chacha_key.into(), &nonce.into());
    cipher.apply_keystream(&mut intermediate);

    intermediate
}

/// Generate scratch pad for decryption at a historical offset.
///
/// When decrypting with history, we need the scratch from that state.
pub fn generate_scratch_at_offset(chain: &Chain, salt: &[u8; 32], offset: usize) -> Vec<u8> {
    let key_index = CURRENT_KEY_INDEX.saturating_sub(offset);
    if key_index < HISTORY_LINKS {
        // Too far back in history
        return vec![0u8; L1_SIZE];
    }

    let mut scratch = vec![0u8; L1_SIZE];

    // Initialize from historical key XOR salt
    let mut state = [0u8; 32];
    for i in 0..32 {
        state[i] = chain.links[key_index][i] ^ salt[i];
    }
    scratch[0..32].copy_from_slice(&state);

    // Fill with sequential hashing
    for i in (32..L1_SIZE).step_by(32) {
        state = smear_hash(&scratch[i - 32..i]);
        scratch[i..i + 32].copy_from_slice(&state);
    }

    // Data-dependent mixing rounds
    for _round in 0..L1_ROUNDS {
        for i in (32..L1_SIZE).step_by(32) {
            let read_idx = (u32::from_le_bytes(scratch[i..i + 4].try_into().unwrap()) as usize)
                % (i / 32)
                * 32;

            let mut mix_input = [0u8; 64];
            mix_input[0..32].copy_from_slice(&scratch[i - 32..i]);
            mix_input[32..64].copy_from_slice(&scratch[read_idx..read_idx + 32]);

            let mixed = smear_hash(&mix_input);
            scratch[i..i + 32].copy_from_slice(&mixed);
        }
    }

    scratch
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Constant-time equality comparison
fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_chain() -> Chain {
        // Create 8KB of test data for active portion
        let bytes: Vec<u8> = (0..ACTIVE_LINKS * LINK_SIZE)
            .map(|i| (i % 256) as u8)
            .collect();
        Chain::from_bytes(&bytes).unwrap()
    }

    #[test]
    fn test_chain_from_bytes() {
        let chain = make_test_chain();

        // History [0..256) should be zeros
        assert_eq!(chain.link(0).unwrap(), &[0u8; 32]);
        assert_eq!(chain.link(255).unwrap(), &[0u8; 32]);

        // Active [256..512) should have our test data
        // First active link at [256] has bytes 0..31
        assert_eq!(chain.link(256).unwrap()[0], 0);
        assert_eq!(chain.link(256).unwrap()[31], 31);

        // Current key at [511]
        let expected_start = (255 * 32) % 256; // Last 32-byte chunk
        assert_eq!(chain.current_key()[0], expected_start as u8);
    }

    #[test]
    fn test_chain_to_bytes_roundtrip() {
        let chain = make_test_chain();
        let full_bytes = chain.to_bytes();
        assert_eq!(full_bytes.len(), CHAIN_SIZE);

        let restored = Chain::from_full_bytes(&full_bytes).unwrap();
        assert_eq!(chain.to_bytes(), restored.to_bytes());
    }

    #[test]
    fn test_chain_advance() {
        let mut chain = make_test_chain();

        // Save original values
        let original_key = *chain.current_key();
        let original_link256 = *chain.link(256).unwrap();

        // Advance (no bidirectional entropy)
        let eagle_time = vsf::datetime_to_eagle_time(chrono::Utc::now());
        let plaintext_hash = [0xAA; 32];
        chain.advance(&eagle_time, &plaintext_hash, None);

        // Key should change (new derived value)
        assert_ne!(chain.current_key(), &original_key);

        // Old [256] should now be at [255] (shifted into history)
        assert_eq!(chain.link(255).unwrap(), &original_link256);
    }

    #[test]
    fn test_chain_advance_deterministic() {
        let mut chain1 = make_test_chain();
        let mut chain2 = make_test_chain();

        let eagle_time = vsf::datetime_to_eagle_time(chrono::Utc::now());
        let plaintext_hash = [42u8; 32];

        chain1.advance(&eagle_time, &plaintext_hash, None);
        chain2.advance(&eagle_time, &plaintext_hash, None);

        assert_eq!(chain1.current_key(), chain2.current_key());
        assert_eq!(chain1.to_bytes(), chain2.to_bytes());
    }

    #[test]
    fn test_chain_advance_bidirectional_entropy() {
        let mut chain1 = make_test_chain();
        let mut chain2 = make_test_chain();
        let mut chain3 = make_test_chain();

        let eagle_time = vsf::datetime_to_eagle_time(chrono::Utc::now());
        let plaintext_hash = [42u8; 32];

        // Advance without their_plaintext
        chain1.advance(&eagle_time, &plaintext_hash, None);

        // Advance with their_plaintext
        chain2.advance(&eagle_time, &plaintext_hash, Some(b"their message content"));

        // Advance with different their_plaintext
        chain3.advance(&eagle_time, &plaintext_hash, Some(b"different message"));

        // All three should produce different keys
        assert_ne!(chain1.current_key(), chain2.current_key());
        assert_ne!(chain1.current_key(), chain3.current_key());
        assert_ne!(chain2.current_key(), chain3.current_key());
    }

    #[test]
    fn test_chain_advance_bidirectional_deterministic() {
        let mut chain1 = make_test_chain();
        let mut chain2 = make_test_chain();

        let eagle_time = vsf::datetime_to_eagle_time(chrono::Utc::now());
        let plaintext_hash = [42u8; 32];
        let their_plaintext = b"their message for entropy";

        chain1.advance(&eagle_time, &plaintext_hash, Some(their_plaintext));
        chain2.advance(&eagle_time, &plaintext_hash, Some(their_plaintext));

        // Same inputs = same output (deterministic)
        assert_eq!(chain1.current_key(), chain2.current_key());
        assert_eq!(chain1.to_bytes(), chain2.to_bytes());
    }

    #[test]
    fn test_derive_salt() {
        let chain = make_test_chain();

        // First message: empty prev
        let salt1 = derive_salt(&[], &chain);

        // Second message: some prev content
        let salt2 = derive_salt(b"Hello world", &chain);

        // Should be different
        assert_ne!(salt1, salt2);

        // Should be deterministic
        let salt1_again = derive_salt(&[], &chain);
        assert_eq!(salt1, salt1_again);
    }

    #[test]
    fn test_generate_scratch() {
        let chain = make_test_chain();
        let salt = [0u8; 32];

        let scratch = generate_scratch(&chain, &salt);

        assert_eq!(scratch.len(), L1_SIZE);

        // Should be deterministic
        let scratch2 = generate_scratch(&chain, &salt);
        assert_eq!(scratch, scratch2);

        // Different salt = different scratch
        let salt2 = [1u8; 32];
        let scratch3 = generate_scratch(&chain, &salt2);
        assert_ne!(scratch, scratch3);
    }

    #[test]
    fn test_confirmation_smear() {
        let chain = make_test_chain();

        let smear1 = generate_confirmation_smear(b"Hello", &chain);
        let smear2 = generate_confirmation_smear(b"World", &chain);

        // Different messages = different smears
        assert_ne!(smear1, smear2);

        // Deterministic
        let smear1_again = generate_confirmation_smear(b"Hello", &chain);
        assert_eq!(smear1, smear1_again);
    }

    #[test]
    fn test_ack_proof() {
        let chain = make_test_chain();
        let eagle_time = vsf::datetime_to_eagle_time(chrono::Utc::now());
        let plaintext_hash = [0xBB; 32];

        let proof = generate_ack_proof(&eagle_time, &plaintext_hash, &chain);

        // Should verify
        assert!(verify_ack_proof(
            &eagle_time,
            &plaintext_hash,
            &chain,
            &proof
        ));

        // Wrong hash should fail
        let wrong_hash = [0xCC; 32];
        assert!(!verify_ack_proof(&eagle_time, &wrong_hash, &chain, &proof));
    }

    #[test]
    fn test_encrypt_decrypt_layers() {
        let chain = make_test_chain();
        let salt = derive_salt(&[], &chain);
        let scratch = generate_scratch(&chain, &salt);
        let eagle_time = vsf::datetime_to_eagle_time(chrono::Utc::now());

        let plaintext = b"Hello, secure world!";
        let ciphertext = encrypt_layers(plaintext, &chain, &scratch, &eagle_time);

        // Should be different from plaintext
        assert_ne!(&ciphertext[..], plaintext);

        // Should decrypt correctly
        let decrypted = decrypt_layers(
            &ciphertext,
            &chain,
            CURRENT_KEY_INDEX,
            &scratch,
            &eagle_time,
        );
        assert_eq!(&decrypted[..], plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_with_chain_advance() {
        let mut sender = make_test_chain();
        let mut receiver = make_test_chain();

        let mut prev_plaintext: Vec<u8> = vec![];

        for i in 0..3 {
            let message = format!("Message {}", i);
            let eagle_time = vsf::datetime_to_eagle_time(chrono::Utc::now());

            // Derive salt from previous plaintext
            let salt = derive_salt(&prev_plaintext, &sender);
            let scratch = generate_scratch(&sender, &salt);

            // Encrypt
            let ciphertext = encrypt_layers(message.as_bytes(), &sender, &scratch, &eagle_time);

            // Receiver derives same salt (they have prev_plaintext from decrypting previous)
            let rx_salt = derive_salt(&prev_plaintext, &receiver);
            let rx_scratch = generate_scratch(&receiver, &rx_salt);

            // Decrypt
            let decrypted = decrypt_layers(
                &ciphertext,
                &receiver,
                CURRENT_KEY_INDEX,
                &rx_scratch,
                &eagle_time,
            );
            assert_eq!(&decrypted[..], message.as_bytes());

            // Compute plaintext hash for ACK
            let plaintext_hash = *blake3::hash(&decrypted).as_bytes();

            // Both advance (no bidirectional entropy in this test)
            sender.advance(&eagle_time, &plaintext_hash, None);
            receiver.advance(&eagle_time, &plaintext_hash, None);

            // Update prev_plaintext for next iteration
            prev_plaintext = decrypted;
        }
    }
}
