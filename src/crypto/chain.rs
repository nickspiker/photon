//! Rolling chain encryption for messages.
//!
//! Implements the rolling chain encryption specified in CLUTCH.md sections 5-6:
//! - L1 memory-hard scratch generation
//! - Dual PRNG for salt diversity (ChaCha20Rng + Pcg64)
//! - ChaCha20-Poly1305 AEAD encryption
//! - Chain state advancement

use crate::types::{EncryptedMessage, Message, Seed};
use blake3::Hasher;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;
use rand_pcg::Pcg64;
use thiserror::Error;

/// L1 scratch buffer size - fits in L1 cache
const L1_SIZE: usize = 32_768; // 32KB

/// Number of rounds for L1 scratch generation
const L1_ROUNDS: usize = 3; // ~1-10ms

#[derive(Debug, Error)]
pub enum ChainError {
    #[error("Sequence number mismatch: expected {expected}, got {actual}")]
    SequenceMismatch { expected: u64, actual: u64 },

    #[error("Decryption failed")]
    DecryptionFailed,

    #[error("Invalid message")]
    InvalidMessage,

    #[error("Chain not initialized")]
    NotInitialized,

    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),
}

pub type Result<T> = std::result::Result<T, ChainError>;

/// Dual PRNG for salt generation - two independent algorithms for diversity
#[derive(Clone)]
struct DualPrng {
    chacha_rng: ChaCha20Rng, // Cryptographically secure PRNG
    pcg_rng: Pcg64,          // Fast, different structure
}

impl DualPrng {
    /// Initialize dual PRNG from seed and first message hash
    fn new(clutch_seed: &[u8; 32], first_message_hash: &[u8; 32]) -> Self {
        // Derive separate seeds for each PRNG
        let chacha_seed = {
            let mut hasher = Hasher::new();
            hasher.update(clutch_seed);
            hasher.update(first_message_hash);
            hasher.update(b"chacha_rng_seed");
            *hasher.finalize().as_bytes()
        };

        let pcg_seed = {
            let mut hasher = Hasher::new();
            hasher.update(clutch_seed);
            hasher.update(first_message_hash);
            hasher.update(b"pcg_rng_seed");
            let hash = hasher.finalize();
            // Pcg64 needs 128-bit seed (16 bytes)
            let mut seed = [0u8; 32];
            seed.copy_from_slice(hash.as_bytes());
            seed
        };

        Self {
            chacha_rng: ChaCha20Rng::from_seed(chacha_seed),
            pcg_rng: Pcg64::from_seed(pcg_seed),
        }
    }

    /// Generate 64-byte salt from both PRNGs (32 bytes each)
    fn generate_salt(&mut self) -> [u8; 64] {
        let mut salt = [0u8; 64];
        self.chacha_rng.fill_bytes(&mut salt[..32]); // Bytes 0-31 from ChaCha20
        self.pcg_rng.fill_bytes(&mut salt[32..]); // Bytes 32-63 from Pcg64
        salt
    }
}

/// Generate L1 memory-hard scratch buffer
///
/// This is cache-hostile and provides resistance against precomputation attacks.
/// Uses data-dependent reads to ensure ASIC-resistance.
fn generate_l1_scratch(chain_state: &[u8; 32]) -> Vec<u8> {
    let mut scratch = vec![0u8; L1_SIZE];

    // Initialize first 32 bytes with chain state
    scratch[..32].copy_from_slice(chain_state);

    for _round in 0..L1_ROUNDS {
        // Sequential fill with data-dependent reads (cache-hostile)
        for i in (32..L1_SIZE).step_by(32) {
            // Hash from previous position
            let prev_hash = blake3::hash(&scratch[i - 32..i]);

            // Data-dependent read position (ASIC-resistant)
            let read_pos =
                (u32::from_le_bytes(prev_hash.as_bytes()[0..4].try_into().unwrap()) as usize) % i;

            // Hash from random earlier position
            let read_end = (read_pos + 32).min(scratch.len());
            let chunk_hash = blake3::hash(&scratch[read_pos..read_end]);

            // Mix with round and position
            let mut hasher = Hasher::new();
            hasher.update(chunk_hash.as_bytes());
            hasher.update(&(i as u64).to_le_bytes());
            let mixed = hasher.finalize();

            scratch[i..i + 32].copy_from_slice(mixed.as_bytes());
        }
    }

    scratch
}

#[derive(Clone)]
pub struct MessageChain {
    /// Current chain state (32 bytes)
    state: [u8; 32],
    /// Original seed (for PRNG initialization)
    seed: [u8; 32],
    /// Send sequence number
    send_sequence: u64,
    /// Receive sequence number
    receive_sequence: u64,
    /// Dual PRNG (initialized on first message)
    prng: Option<DualPrng>,
    /// Out-of-order messages waiting to be processed
    pending_messages: Vec<(u64, Vec<u8>, [u8; 64])>, // (sequence, ciphertext, salt)
    /// Sent messages awaiting ACK (sequence, ciphertext, plaintext_hash)
    sent_messages: Vec<(u64, Vec<u8>, [u8; 32])>,
    /// Precomputed L1 scratch for next send (background computation)
    precomputed_scratch: Option<Vec<u8>>,
}

impl MessageChain {
    /// Create a new message chain from the CLUTCH seed
    pub fn new(seed: Seed) -> Self {
        let state = seed.derive_chain_state();
        Self {
            state,
            seed: *seed.as_bytes(),
            send_sequence: 0,
            receive_sequence: 0,
            prng: None,
            pending_messages: Vec::new(),
            sent_messages: Vec::new(),
            precomputed_scratch: None,
        }
    }

    /// Precompute L1 scratch for next send (call in background thread)
    pub fn precompute_scratch(&mut self) {
        if self.precomputed_scratch.is_none() {
            self.precomputed_scratch = Some(generate_l1_scratch(&self.state));
        }
    }

    /// Encrypt a message payload
    pub fn encrypt(&mut self, payload: &[u8]) -> Result<EncryptedMessage> {
        let message = Message::new(self.send_sequence, payload.to_vec());
        let serialized = message.to_vsf_bytes();

        // 0. Generate or use precomputed L1 scratch
        let scratch = self
            .precomputed_scratch
            .take()
            .unwrap_or_else(|| generate_l1_scratch(&self.state));

        // 1. Derive message key from scratch
        let message_key = {
            let mut hasher = Hasher::new();
            hasher.update(&scratch);
            hasher.update(&self.send_sequence.to_le_bytes());
            hasher.update(b"message_key");
            hasher.finalize()
        };

        // 2. Initialize PRNG if this is first message
        let plaintext_hash = *blake3::hash(&serialized).as_bytes();
        if self.prng.is_none() {
            self.prng = Some(DualPrng::new(&self.seed, &plaintext_hash));
        }

        // 3. Generate dual PRNG salt (64 bytes)
        let salt = self.prng.as_mut().unwrap().generate_salt();

        // 4. Derive encryption key with salt
        let encryption_key = {
            let mut hasher = Hasher::new();
            hasher.update(message_key.as_bytes());
            hasher.update(&salt);
            hasher.update(b"encryption");
            hasher.finalize()
        };

        // 5. Encrypt with ChaCha20-Poly1305
        let cipher = ChaCha20Poly1305::new_from_slice(&encryption_key.as_bytes()[..32])
            .map_err(|e| ChainError::EncryptionFailed(e.to_string()))?;

        // Nonce from sequence number (12 bytes)
        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[..8].copy_from_slice(&self.send_sequence.to_le_bytes());
        let nonce: Nonce = nonce_bytes.into();

        let ciphertext = cipher
            .encrypt(&nonce, serialized.as_ref())
            .map_err(|e| ChainError::EncryptionFailed(e.to_string()))?;

        // 6. Store for potential retransmission
        let seq = self.send_sequence;
        self.sent_messages
            .push((seq, ciphertext.clone(), plaintext_hash));
        self.send_sequence += 1;

        // 7. Advance chain state (sender and receiver must stay synchronized)
        self.advance_state(&plaintext_hash);

        // 8. Precompute scratch for next message (uses new state)
        self.precomputed_scratch = Some(generate_l1_scratch(&self.state));

        Ok(EncryptedMessage {
            sequence: seq,
            salt,
            ciphertext,
        })
    }

    /// Decrypt a received message
    pub fn decrypt(&mut self, encrypted: &EncryptedMessage) -> Result<Message> {
        // Check sequence number
        if encrypted.sequence != self.receive_sequence {
            if encrypted.sequence > self.receive_sequence {
                // Store for later processing
                self.pending_messages.push((
                    encrypted.sequence,
                    encrypted.ciphertext.clone(),
                    encrypted.salt,
                ));
                return Err(ChainError::SequenceMismatch {
                    expected: self.receive_sequence,
                    actual: encrypted.sequence,
                });
            } else {
                // Old message, reject
                return Err(ChainError::SequenceMismatch {
                    expected: self.receive_sequence,
                    actual: encrypted.sequence,
                });
            }
        }

        // Decrypt the message
        let message = self.decrypt_internal(
            &encrypted.ciphertext,
            &encrypted.salt,
            self.receive_sequence,
        )?;

        // Advance chain state
        let plaintext_hash = blake3::hash(&message.to_vsf_bytes());
        self.advance_state(plaintext_hash.as_bytes());
        self.receive_sequence += 1;

        // Note: Receiver does NOT advance PRNG - the sender generates the salt
        // and includes it in the message. We just need to initialize the PRNG
        // if this is our first interaction (for when we become the sender).
        if self.prng.is_none() {
            // First message seen - initialize PRNG with the plaintext hash
            self.prng = Some(DualPrng::new(&self.seed, plaintext_hash.as_bytes()));
        }

        // Process any pending out-of-order messages
        self.process_pending_messages();

        Ok(message)
    }

    /// Internal decryption helper
    fn decrypt_internal(
        &self,
        ciphertext: &[u8],
        salt: &[u8; 64],
        sequence: u64,
    ) -> Result<Message> {
        // 1. Generate L1 scratch (deterministic from chain state)
        let scratch = generate_l1_scratch(&self.state);

        // 2. Derive message key
        let message_key = {
            let mut hasher = Hasher::new();
            hasher.update(&scratch);
            hasher.update(&sequence.to_le_bytes());
            hasher.update(b"message_key");
            hasher.finalize()
        };

        // 3. Derive encryption key with received salt
        let encryption_key = {
            let mut hasher = Hasher::new();
            hasher.update(message_key.as_bytes());
            hasher.update(salt);
            hasher.update(b"encryption");
            hasher.finalize()
        };

        // 4. Decrypt with ChaCha20-Poly1305
        let cipher = ChaCha20Poly1305::new_from_slice(&encryption_key.as_bytes()[..32])
            .map_err(|_| ChainError::DecryptionFailed)?;

        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[..8].copy_from_slice(&sequence.to_le_bytes());
        let nonce: Nonce = nonce_bytes.into();

        let plaintext = cipher
            .decrypt(&nonce, ciphertext)
            .map_err(|_| ChainError::DecryptionFailed)?;

        // Parse message from plaintext
        Message::from_vsf_bytes(&plaintext).map_err(|_| ChainError::InvalidMessage)
    }

    /// Handle ACK for a sent message - just clears from retransmit queue
    ///
    /// Note: Chain state was already advanced when we encrypted the message.
    /// ACK just confirms we can stop holding it for retransmission.
    pub fn receive_ack(&mut self, sequence: u64) {
        if let Some(idx) = self
            .sent_messages
            .iter()
            .position(|(s, _, _)| *s == sequence)
        {
            self.sent_messages.swap_remove(idx);
        }
    }

    /// Advance chain state
    fn advance_state(&mut self, plaintext_hash: &[u8; 32]) {
        // Generate scratch for state advancement
        let scratch = generate_l1_scratch(&self.state);
        let scratch_hash = blake3::hash(&scratch);

        let mut hasher = Hasher::new();
        hasher.update(&self.state);
        hasher.update(plaintext_hash);
        hasher.update(scratch_hash.as_bytes());
        self.state = *hasher.finalize().as_bytes();

        // Invalidate precomputed scratch (state changed)
        self.precomputed_scratch = None;
    }

    /// Process pending out-of-order messages
    fn process_pending_messages(&mut self) {
        while let Some(idx) = self
            .pending_messages
            .iter()
            .position(|(s, _, _)| *s == self.receive_sequence)
        {
            let (_, ciphertext, salt) = self.pending_messages.swap_remove(idx);

            match self.decrypt_internal(&ciphertext, &salt, self.receive_sequence) {
                Ok(message) => {
                    let plaintext_hash = blake3::hash(&message.to_vsf_bytes());
                    self.advance_state(plaintext_hash.as_bytes());
                    self.receive_sequence += 1;
                    // Note: Don't advance PRNG - receiver doesn't control salt
                }
                Err(_) => break, // Invalid message - stop processing
            }
        }
    }

    /// Get current send sequence number
    pub fn current_send_sequence(&self) -> u64 {
        self.send_sequence
    }

    /// Get current receive sequence number
    pub fn current_receive_sequence(&self) -> u64 {
        self.receive_sequence
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_l1_scratch_deterministic() {
        let state = [42u8; 32];
        let scratch1 = generate_l1_scratch(&state);
        let scratch2 = generate_l1_scratch(&state);
        assert_eq!(scratch1, scratch2);
        assert_eq!(scratch1.len(), L1_SIZE);
    }

    #[test]
    fn test_l1_scratch_different_states() {
        let state1 = [1u8; 32];
        let state2 = [2u8; 32];
        let scratch1 = generate_l1_scratch(&state1);
        let scratch2 = generate_l1_scratch(&state2);
        assert_ne!(scratch1, scratch2);
    }

    #[test]
    fn test_dual_prng_deterministic() {
        let seed = [1u8; 32];
        let msg_hash = [2u8; 32];

        let mut prng1 = DualPrng::new(&seed, &msg_hash);
        let mut prng2 = DualPrng::new(&seed, &msg_hash);

        let salt1 = prng1.generate_salt();
        let salt2 = prng2.generate_salt();
        assert_eq!(salt1, salt2);

        // Subsequent calls also match
        let salt1_b = prng1.generate_salt();
        let salt2_b = prng2.generate_salt();
        assert_eq!(salt1_b, salt2_b);
        assert_ne!(salt1, salt1_b); // Should be different from first
    }

    #[test]
    fn test_chain_encrypt_decrypt_roundtrip() {
        let seed = Seed::from_bytes([0x42u8; 32]);

        let mut sender_chain = MessageChain::new(seed.clone());
        let mut receiver_chain = MessageChain::new(seed);

        let payload = b"Hello, secure world!";
        let encrypted = sender_chain.encrypt(payload).unwrap();

        // Verify salt is present and 64 bytes
        assert_eq!(encrypted.salt.len(), 64);

        let decrypted = receiver_chain.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted.payload, payload);
    }

    #[test]
    fn test_chain_multiple_messages() {
        let seed = Seed::from_bytes([0x42u8; 32]);

        let mut sender_chain = MessageChain::new(seed.clone());
        let mut receiver_chain = MessageChain::new(seed);

        for i in 0..5 {
            let payload = format!("Message {}", i);
            let encrypted = sender_chain.encrypt(payload.as_bytes()).unwrap();
            let decrypted = receiver_chain.decrypt(&encrypted).unwrap();
            assert_eq!(decrypted.payload, payload.as_bytes());
        }
    }

    #[test]
    fn test_chain_wrong_sequence_rejected() {
        let seed = Seed::from_bytes([0x42u8; 32]);

        let mut sender_chain = MessageChain::new(seed.clone());
        let mut receiver_chain = MessageChain::new(seed);

        // Send two messages
        let _enc1 = sender_chain.encrypt(b"first").unwrap();
        let enc2 = sender_chain.encrypt(b"second").unwrap();

        // Try to decrypt second message first (wrong sequence)
        let result = receiver_chain.decrypt(&enc2);
        assert!(matches!(result, Err(ChainError::SequenceMismatch { .. })));
    }
}
