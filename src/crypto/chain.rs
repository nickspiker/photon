//! Rolling chain encryption for messages.
//!
//! Implements dual-pad rolling chain encryption with ACK weaving:
//! - Two 1MB pads (send_pad, recv_pad) - asymmetric based on handle hash ordering
//! - FIFO rotation: push hash on top, pop 32B from bottom
//! - Independent rotation: recv_pad rotates on decrypt, send_pad rotates on ACK receipt
//! - Weave verification: ACK contains plaintext_hash that must match what we sent
//! - ChaCha20-Poly1305 AEAD encryption
//! - Tamper-evident conversation history

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

    #[error("Pads not initialized")]
    PadsNotInitialized,

    #[error("ACK hash mismatch: expected {expected:?}, received {received:?}")]
    AckHashMismatch {
        expected: [u8; 32],
        received: [u8; 32],
    },
}

pub type Result<T> = std::result::Result<T, ChainError>;

/// Compute weave hash: binds both conversation directions into rotation
///
/// weave_hash = BLAKE3(their_plaintext_hash || their_last_acked_hash)
fn compute_weave_hash(their_hash: &[u8; 32], their_last_acked: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(their_hash);
    hasher.update(their_last_acked);
    *hasher.finalize().as_bytes()
}

/// Rotate pad using FIFO queue: push 32 bytes on top, pop 32 bytes from bottom
///
/// This creates a rolling window where recent hashes are at the front and old
/// hashes are evicted. Both parties perform identical rotations to stay synchronized.
fn rotate_pad_with_hash(pad: &mut Vec<u8>, hash: &[u8; 32]) {
    // Push new hash at position 0 (top of queue)
    pad.splice(0..0, hash.iter().cloned());

    // Pop 32 bytes from end (bottom of queue)
    let new_len = pad.len() - 32;
    pad.truncate(new_len);
}

/// Sent message structure for retransmit queue
#[derive(Clone, Debug)]
struct SentMessage {
    sequence: u64,
    ciphertext: Vec<u8>,
    plaintext_hash: [u8; 32], // Needed for pad rotation when ACK arrives
    timestamp: f64,
}

/// Pending message structure for out-of-order buffer
#[derive(Clone, Debug)]
struct PendingMessage {
    sequence: u64,
    ciphertext: Vec<u8>,
}

#[derive(Clone)]
pub struct MessageChain {
    /// 1MB rolling pad for sending messages (rotates on ACK receipt only)
    send_pad: Vec<u8>,
    /// 1MB rolling pad for receiving messages (rotates on decrypt only)
    recv_pad: Vec<u8>,
    /// Send sequence number
    send_sequence: u64,
    /// Receive sequence number
    receive_sequence: u64,
    /// Out-of-order messages waiting to be processed
    pending_messages: Vec<PendingMessage>,
    /// Sent messages awaiting ACK
    sent_messages: Vec<SentMessage>,
    /// Hash of our most recent message that was ACK'd by peer (for weave binding)
    last_acked_sent_hash: [u8; 32],
}

impl MessageChain {
    /// Create a new message chain from dual pads (from CLUTCH avalanche mixer)
    pub fn new(send_pad: Vec<u8>, recv_pad: Vec<u8>) -> Self {
        Self {
            send_pad,
            recv_pad,
            send_sequence: 0,
            receive_sequence: 0,
            pending_messages: Vec::new(),
            sent_messages: Vec::new(),
            last_acked_sent_hash: [0u8; 32], // Bootstrap: zeros until first ACK received
        }
    }

    /// Encrypt a message payload
    ///
    /// Derives key from send_pad, encrypts, stores plaintext_hash for ACK rotation.
    /// Does NOT rotate send_pad yet - waits for ACK.
    pub fn encrypt(&mut self, payload: &[u8]) -> Result<EncryptedMessage> {
        let message = Message::new(self.send_sequence, payload.to_vec());
        let serialized = message.to_vsf_bytes();

        // 0. Derive encryption key from current send_pad position (first 32 bytes)
        let key_material = &self.send_pad[0..32];
        let encryption_key = blake3::derive_key("photon.message.v1", key_material);

        // 1. Encrypt with ChaCha20-Poly1305
        let cipher = ChaCha20Poly1305::new_from_slice(&encryption_key[..32])
            .map_err(|e| ChainError::EncryptionFailed(e.to_string()))?;

        // Nonce from sequence number (12 bytes)
        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[..8].copy_from_slice(&self.send_sequence.to_le_bytes());
        let nonce: Nonce = nonce_bytes.into();

        let ciphertext = cipher
            .encrypt(&nonce, serialized.as_ref())
            .map_err(|e| ChainError::EncryptionFailed(e.to_string()))?;

        // 2. Compute plaintext hash (will be used for rotation when ACK arrives)
        let plaintext_hash = *blake3::hash(&serialized).as_bytes();

        // 3. Store in retransmit queue with plaintext_hash
        let seq = self.send_sequence;
        self.sent_messages.push(SentMessage {
            sequence: seq,
            ciphertext: ciphertext.clone(),
            plaintext_hash,
            timestamp: vsf::eagle_time_nanos(),
        });

        self.send_sequence += 1;

        // DO NOT rotate send_pad yet - wait for ACK

        Ok(EncryptedMessage {
            sequence: seq,
            ciphertext,
        })
    }

    /// Decrypt a received message
    ///
    /// Derives key from recv_pad, decrypts, rotates recv_pad immediately.
    /// Returns plaintext_hash for ACK signing (sent back to prove we decrypted correctly).
    pub fn decrypt(&mut self, encrypted: &EncryptedMessage) -> Result<(Message, [u8; 32])> {
        // Check sequence number
        if encrypted.sequence != self.receive_sequence {
            if encrypted.sequence > self.receive_sequence {
                // Store for later processing
                self.pending_messages.push(PendingMessage {
                    sequence: encrypted.sequence,
                    ciphertext: encrypted.ciphertext.clone(),
                });
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
        let (message, plaintext_hash) =
            self.decrypt_internal(&encrypted.ciphertext, self.receive_sequence)?;

        // ROTATE recv_pad immediately (we successfully decrypted)
        // send_pad does NOT rotate here - it only rotates when WE receive an ACK for OUR message
        rotate_pad_with_hash(&mut self.recv_pad, &plaintext_hash);

        self.receive_sequence += 1;

        // Process any pending out-of-order messages
        self.process_pending_messages();

        Ok((message, plaintext_hash))
    }

    /// Internal decryption helper
    fn decrypt_internal(&self, ciphertext: &[u8], sequence: u64) -> Result<(Message, [u8; 32])> {
        // 0. Derive encryption key from current recv_pad position (first 32 bytes)
        let key_material = &self.recv_pad[0..32];
        let encryption_key = blake3::derive_key("photon.message.v1", key_material);

        // 1. Decrypt with ChaCha20-Poly1305
        let cipher = ChaCha20Poly1305::new_from_slice(&encryption_key[..32])
            .map_err(|_| ChainError::DecryptionFailed)?;

        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[..8].copy_from_slice(&sequence.to_le_bytes());
        let nonce: Nonce = nonce_bytes.into();

        let plaintext = cipher
            .decrypt(&nonce, ciphertext)
            .map_err(|_| ChainError::DecryptionFailed)?;

        // 2. Parse message from plaintext
        let message =
            Message::from_vsf_bytes(&plaintext).map_err(|_| ChainError::InvalidMessage)?;

        // 3. Compute plaintext hash (for pad rotation)
        let plaintext_hash = *blake3::hash(&plaintext).as_bytes();

        Ok((message, plaintext_hash))
    }

    /// Handle ACK for a sent message with bidirectional weave binding
    ///
    /// ACK contains two hashes:
    /// - `their_hash`: proves they decrypted our message correctly (must match what we sent)
    /// - `their_last_acked`: their most recent msg we ACK'd (weaves their chain into ours)
    ///
    /// Rotation uses combined weave hash: BLAKE3(their_hash || their_last_acked)
    pub fn receive_ack(
        &mut self,
        sequence: u64,
        their_hash: &[u8; 32],
        their_last_acked: &[u8; 32],
    ) -> Result<()> {
        // Find sent message
        let sent_msg_idx = self
            .sent_messages
            .iter()
            .position(|m| m.sequence == sequence)
            .ok_or(ChainError::UnknownAck(sequence))?;

        let sent_msg = &self.sent_messages[sent_msg_idx];

        // WEAVE CHECK: verify they decrypted the right message
        if their_hash != &sent_msg.plaintext_hash {
            return Err(ChainError::AckHashMismatch {
                expected: sent_msg.plaintext_hash,
                received: *their_hash,
            });
        }

        // Compute weave hash: binds both conversation directions
        let weave_hash = compute_weave_hash(their_hash, their_last_acked);

        // ROTATE send_pad with the combined weave hash
        rotate_pad_with_hash(&mut self.send_pad, &weave_hash);

        // Update our last_acked_sent_hash for future ACKs we send
        self.last_acked_sent_hash = *their_hash;

        // Remove from retransmit queue
        self.sent_messages.swap_remove(sent_msg_idx);

        Ok(())
    }

    /// Get the hash of our most recent ACK'd message (for weave binding in our ACKs)
    pub fn get_last_acked_hash(&self) -> [u8; 32] {
        self.last_acked_sent_hash
    }

    /// Process pending out-of-order messages
    fn process_pending_messages(&mut self) {
        while let Some(idx) = self
            .pending_messages
            .iter()
            .position(|pm| pm.sequence == self.receive_sequence)
        {
            let pending = self.pending_messages.swap_remove(idx);

            match self.decrypt_internal(&pending.ciphertext, self.receive_sequence) {
                Ok((_, plaintext_hash)) => {
                    // Rotate recv_pad only - send_pad rotates when WE receive ACKs
                    rotate_pad_with_hash(&mut self.recv_pad, &plaintext_hash);
                    self.receive_sequence += 1;
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
    fn test_pad_rotation() {
        let mut pad = vec![0u8; 1024]; // Small test pad
        let hash1 = [1u8; 32];
        let hash2 = [2u8; 32];

        let original_end = pad[pad.len() - 32..].to_vec();

        // Rotate with hash1
        rotate_pad_with_hash(&mut pad, &hash1);
        assert_eq!(&pad[0..32], &hash1); // Hash pushed to front
        assert_eq!(pad.len(), 1024); // Size unchanged

        // Rotate with hash2
        rotate_pad_with_hash(&mut pad, &hash2);
        assert_eq!(&pad[0..32], &hash2); // New hash at front
        assert_eq!(&pad[32..64], &hash1); // Old hash shifted back
        assert_ne!(&pad[pad.len() - 32..], &original_end[..]); // End changed (popped)
    }

    #[test]
    fn test_chain_encrypt_decrypt_roundtrip() {
        // Create dual pads (small test pads)
        let send_pad = vec![0xAAu8; 1024];
        let recv_pad = vec![0xBBu8; 1024];

        let mut sender_chain = MessageChain::new(send_pad.clone(), recv_pad.clone());
        let mut receiver_chain = MessageChain::new(recv_pad, send_pad);

        let payload = b"Hello, secure world!";
        let encrypted = sender_chain.encrypt(payload).unwrap();

        let (decrypted, _plaintext_hash) = receiver_chain.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted.payload, payload);
    }

    #[test]
    fn test_chain_multiple_messages() {
        let send_pad = vec![0xAAu8; 1024];
        let recv_pad = vec![0xBBu8; 1024];

        let mut sender_chain = MessageChain::new(send_pad.clone(), recv_pad.clone());
        let mut receiver_chain = MessageChain::new(recv_pad, send_pad);

        for i in 0..5 {
            let payload = format!("Message {}", i);
            let encrypted = sender_chain.encrypt(payload.as_bytes()).unwrap();
            let (decrypted, _) = receiver_chain.decrypt(&encrypted).unwrap();
            assert_eq!(decrypted.payload, payload.as_bytes());
        }
    }

    #[test]
    fn test_chain_wrong_sequence_rejected() {
        let send_pad = vec![0xAAu8; 1024];
        let recv_pad = vec![0xBBu8; 1024];

        let mut sender_chain = MessageChain::new(send_pad.clone(), recv_pad.clone());
        let mut receiver_chain = MessageChain::new(recv_pad, send_pad);

        // Send two messages
        let _enc1 = sender_chain.encrypt(b"first").unwrap();
        let enc2 = sender_chain.encrypt(b"second").unwrap();

        // Try to decrypt second message first (wrong sequence)
        let result = receiver_chain.decrypt(&enc2);
        assert!(matches!(result, Err(ChainError::SequenceMismatch { .. })));
    }
}
