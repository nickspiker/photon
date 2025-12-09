//! Rolling chain encryption for messages.
//!
//! 256-link Chain (8KB):
//! - Fixed-size `[[u8; 32]; 256]` = 8KB per chain
//! - Full rotation: all 256 links shift through memory on each advance
//! - Forces entire 8KB through BLAKE3 for avalanche effect (optimal for SIMD)
//! - Per-participant chains in friendships (N chains for N-party)
//!
//! Encryption uses ChaCha20-Poly1305 AEAD with key derived from current link[0].

use crate::types::{EncryptedMessage, Message};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use thiserror::Error;

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

    #[error("Unknown ACK sequence: {0}")]
    UnknownAck(u64),

    #[error("ACK hash mismatch: expected {expected:?}, received {received:?}")]
    AckHashMismatch {
        expected: [u8; 32],
        received: [u8; 32],
    },

    #[error("Participant not found in friendship")]
    ParticipantNotFound,
}

pub type Result<T> = std::result::Result<T, ChainError>;

/// Number of links in a chain (256 × 32B = 8KB)
pub const CHAIN_LINKS: usize = 256;

/// Size of each link in bytes
pub const LINK_SIZE: usize = 32;

/// Total chain size in bytes (8KB)
pub const CHAIN_SIZE: usize = CHAIN_LINKS * LINK_SIZE;

/// 256-link chain (8KB) - one per participant in a friendship
///
/// Links are 32 bytes each. On advance, ALL links physically rotate through
/// memory (255→drop, 254→255, ..., 0→1) and a new link[0] is derived from:
/// `BLAKE3(old_link[0] || plaintext_hash || full_chain)`
///
/// This forces the entire 8KB through BLAKE3 for maximum avalanche effect,
/// which is optimal for BLAKE3's SIMD operations on contiguous memory.
#[derive(Clone)]
pub struct Chain {
    /// 256 links × 32 bytes = 8KB, stored contiguously for BLAKE3 SIMD
    links: [[u8; 32]; CHAIN_LINKS],
}

impl Chain {
    /// Create a new chain from raw bytes (8KB from CLUTCH avalanche)
    ///
    /// Returns None if bytes.len() != 8192
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != CHAIN_SIZE {
            return None;
        }

        let mut links = [[0u8; 32]; CHAIN_LINKS];
        for (i, chunk) in bytes.chunks_exact(LINK_SIZE).enumerate() {
            links[i].copy_from_slice(chunk);
        }

        Some(Self { links })
    }

    /// Serialize chain to raw bytes (8KB)
    pub fn to_bytes(&self) -> Vec<u8> {
        self.links.as_flattened().to_vec()
    }

    /// Get the current encryption key (always link[0])
    #[inline]
    pub fn current_key(&self) -> &[u8; 32] {
        &self.links[0]
    }

    /// Advance the chain after ACK confirmation.
    ///
    /// Physically rotates ALL 256 links through memory:
    /// - link[255] is dropped
    /// - link[254] → link[255], link[253] → link[254], ..., link[0] → link[1]
    /// - New link[0] = BLAKE3(old_link[0] || plaintext_hash || full_chain)
    ///
    /// This ensures the entire 8KB is processed by BLAKE3 for avalanche effect.
    pub fn advance(&mut self, plaintext_hash: &[u8; 32]) {
        // Save old link[0] before rotation
        let old_top = self.links[0];

        // Rotate all links down (255→drop, 254→255, ..., 0→1)
        // memmove semantics: copy_within handles overlapping regions correctly
        self.links.copy_within(0..CHAIN_LINKS - 1, 1);

        // Derive new link[0] from old top, plaintext hash, and FULL chain
        // The full chain read forces BLAKE3 to process all 8KB for SIMD avalanche
        let mut hasher = blake3::Hasher::new();
        hasher.update(&old_top);
        hasher.update(plaintext_hash);
        hasher.update(self.links.as_flattened()); // Full 8KB for avalanche
        self.links[0] = *hasher.finalize().as_bytes();
    }

    /// Get a reference to link at index (for debugging/inspection)
    pub fn link(&self, index: usize) -> Option<&[u8; 32]> {
        self.links.get(index)
    }

    /// Encrypt a message using this chain's current key.
    ///
    /// Does NOT advance the chain - call advance() only after ACK.
    pub fn encrypt(&self, sequence: u64, payload: &[u8]) -> Result<(EncryptedMessage, [u8; 32])> {
        let message = Message::new(sequence, payload.to_vec());
        let serialized = message.to_vsf_bytes();

        // Derive encryption key from current link[0]
        let encryption_key = blake3::derive_key("photon.chain.v2", self.current_key());

        // Encrypt with ChaCha20-Poly1305
        let cipher = ChaCha20Poly1305::new_from_slice(&encryption_key[..32])
            .map_err(|e| ChainError::EncryptionFailed(e.to_string()))?;

        // Nonce from sequence number (12 bytes)
        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[..8].copy_from_slice(&sequence.to_le_bytes());
        let nonce: Nonce = nonce_bytes.into();

        let ciphertext = cipher
            .encrypt(&nonce, serialized.as_ref())
            .map_err(|e| ChainError::EncryptionFailed(e.to_string()))?;

        // Compute plaintext hash (for ACK verification and chain advancement)
        let plaintext_hash = *blake3::hash(&serialized).as_bytes();

        Ok((
            EncryptedMessage {
                sequence,
                ciphertext,
            },
            plaintext_hash,
        ))
    }

    /// Decrypt a message using this chain's current key.
    ///
    /// Does NOT advance the chain - call advance() only after ACK.
    pub fn decrypt(&self, encrypted: &EncryptedMessage) -> Result<(Message, [u8; 32])> {
        // Derive encryption key from current link[0]
        let encryption_key = blake3::derive_key("photon.chain.v2", self.current_key());

        // Decrypt with ChaCha20-Poly1305
        let cipher = ChaCha20Poly1305::new_from_slice(&encryption_key[..32])
            .map_err(|_| ChainError::DecryptionFailed)?;

        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[..8].copy_from_slice(&encrypted.sequence.to_le_bytes());
        let nonce: Nonce = nonce_bytes.into();

        let plaintext = cipher
            .decrypt(&nonce, encrypted.ciphertext.as_ref())
            .map_err(|_| ChainError::DecryptionFailed)?;

        // Parse message from plaintext
        let message = Message::from_vsf_bytes(&plaintext).map_err(|_| ChainError::InvalidMessage)?;

        // Compute plaintext hash (for ACK and chain advancement)
        let plaintext_hash = *blake3::hash(&plaintext).as_bytes();

        Ok((message, plaintext_hash))
    }
}

impl std::fmt::Debug for Chain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Don't leak key material - just show structure
        write!(f, "Chain {{ {} links, 8KB }}", CHAIN_LINKS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_from_bytes() {
        // Create 8KB of test data
        let bytes: Vec<u8> = (0..CHAIN_SIZE).map(|i| (i % 256) as u8).collect();
        let chain = Chain::from_bytes(&bytes).unwrap();

        // Verify link[0] contains first 32 bytes
        assert_eq!(chain.current_key()[0], 0);
        assert_eq!(chain.current_key()[31], 31);

        // Verify link[1] contains bytes 32-63
        assert_eq!(chain.link(1).unwrap()[0], 32);

        // Wrong size should fail
        assert!(Chain::from_bytes(&[0u8; 100]).is_none());
        assert!(Chain::from_bytes(&[0u8; CHAIN_SIZE + 1]).is_none());
    }

    #[test]
    fn test_chain_to_bytes_roundtrip() {
        let bytes: Vec<u8> = (0..CHAIN_SIZE).map(|i| (i % 256) as u8).collect();
        let chain = Chain::from_bytes(&bytes).unwrap();
        let recovered = chain.to_bytes();
        assert_eq!(bytes, recovered);
    }

    #[test]
    fn test_chain_advance_rotation() {
        let bytes: Vec<u8> = (0..CHAIN_SIZE).map(|i| (i % 256) as u8).collect();
        let mut chain = Chain::from_bytes(&bytes).unwrap();

        // Save original values
        let original_link0 = *chain.current_key();
        let original_link1 = *chain.link(1).unwrap();

        // Advance with a plaintext hash
        let plaintext_hash = [0xAA; 32];
        chain.advance(&plaintext_hash);

        // link[0] should now be the new derived value (different from original)
        assert_ne!(chain.current_key(), &original_link0);

        // link[1] should now contain the OLD link[0] value
        assert_eq!(chain.link(1).unwrap(), &original_link0);

        // link[2] should now contain the OLD link[1] value
        assert_eq!(chain.link(2).unwrap(), &original_link1);
    }

    #[test]
    fn test_chain_advance_avalanche() {
        // Two chains starting with same data
        let bytes: Vec<u8> = (0..CHAIN_SIZE).map(|i| (i % 256) as u8).collect();
        let mut chain1 = Chain::from_bytes(&bytes).unwrap();
        let mut chain2 = Chain::from_bytes(&bytes).unwrap();

        // Advance with slightly different plaintext hashes
        let hash1 = [0u8; 32];
        let mut hash2 = [0u8; 32];
        hash2[0] = 1; // Single bit difference

        chain1.advance(&hash1);
        chain2.advance(&hash2);

        // Should produce completely different link[0] values (avalanche effect)
        let key1 = chain1.current_key();
        let key2 = chain2.current_key();
        assert_ne!(key1, key2);

        // Count bit differences - should be ~50% (128 bits of 256)
        let diff_bits: u32 = key1
            .iter()
            .zip(key2.iter())
            .map(|(a, b)| (a ^ b).count_ones())
            .sum();
        // Expect roughly half the bits to differ (with some tolerance)
        assert!(
            diff_bits > 100,
            "Avalanche effect: {} bits differ",
            diff_bits
        );
        assert!(
            diff_bits < 156,
            "Avalanche effect: {} bits differ",
            diff_bits
        );
    }

    #[test]
    fn test_chain_deterministic() {
        // Same inputs should produce same outputs
        let bytes: Vec<u8> = (0..CHAIN_SIZE).map(|i| (i % 256) as u8).collect();
        let mut chain1 = Chain::from_bytes(&bytes).unwrap();
        let mut chain2 = Chain::from_bytes(&bytes).unwrap();

        let hash = [42u8; 32];
        chain1.advance(&hash);
        chain2.advance(&hash);

        assert_eq!(chain1.current_key(), chain2.current_key());
        assert_eq!(chain1.to_bytes(), chain2.to_bytes());
    }

    #[test]
    fn test_chain_encrypt_decrypt() {
        let bytes: Vec<u8> = (0..CHAIN_SIZE).map(|i| (i % 256) as u8).collect();
        let chain = Chain::from_bytes(&bytes).unwrap();

        let payload = b"Hello, secure world!";
        let (encrypted, plaintext_hash) = chain.encrypt(0, payload).unwrap();

        // Same chain (same key) should decrypt successfully
        let (decrypted, decrypted_hash) = chain.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted.payload, payload);
        assert_eq!(plaintext_hash, decrypted_hash);
    }

    #[test]
    fn test_chain_encrypt_decrypt_sequence() {
        let bytes: Vec<u8> = (0..CHAIN_SIZE).map(|i| (i % 256) as u8).collect();
        let mut sender = Chain::from_bytes(&bytes).unwrap();
        let mut receiver = Chain::from_bytes(&bytes).unwrap();

        // Send multiple messages with ACK advancement
        for seq in 0..5u64 {
            let payload = format!("Message {}", seq);

            // Sender encrypts
            let (encrypted, plaintext_hash) = sender.encrypt(seq, payload.as_bytes()).unwrap();

            // Receiver decrypts
            let (decrypted, rx_hash) = receiver.decrypt(&encrypted).unwrap();
            assert_eq!(decrypted.payload, payload.as_bytes());
            assert_eq!(plaintext_hash, rx_hash);

            // Both advance their chains (simulating ACK)
            sender.advance(&plaintext_hash);
            receiver.advance(&plaintext_hash);
        }
    }
}
