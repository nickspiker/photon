use crate::types::{EncryptedMessage, Message, Seed};
use blake3::Hasher;
use std::collections::HashMap;
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
}

pub type Result<T> = std::result::Result<T, ChainError>;

#[derive(Clone)]
pub struct MessageChain {
    state: [u8; 32],
    send_sequence: u64,
    receive_sequence: u64,
    pending_messages: HashMap<u64, Vec<u8>>,
    sent_messages: HashMap<u64, Vec<u8>>, // Store sent ciphertexts until ACK'd
}

impl MessageChain {
    pub fn new(seed: Seed) -> Self {
        Self {
            state: seed.derive_chain_state(),
            send_sequence: 0,
            receive_sequence: 0,
            pending_messages: HashMap::new(),
            sent_messages: HashMap::new(),
        }
    }

    pub fn encrypt(&mut self, payload: &[u8]) -> EncryptedMessage {
        let message = Message::new(self.send_sequence, payload.to_vec());
        let serialized = message.to_vsf_bytes();

        // TODO: Proper key derivation - for now just XOR with state
        let mut ciphertext = serialized.clone();
        for (i, byte) in ciphertext.iter_mut().enumerate() {
            *byte ^= self.state[i % 32];
        }

        // Store ciphertext for when we get ACK
        let seq = self.send_sequence;
        self.sent_messages.insert(seq, ciphertext.clone());
        self.send_sequence += 1;

        EncryptedMessage {
            sequence: seq,
            ciphertext,
        }
    }

    pub fn decrypt(&mut self, encrypted: &EncryptedMessage) -> Result<Message> {
        // Check sequence number
        if encrypted.sequence != self.receive_sequence {
            if encrypted.sequence > self.receive_sequence {
                // Store for later
                self.pending_messages
                    .insert(encrypted.sequence, encrypted.ciphertext.clone());
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

        // TODO: Proper decryption - for now just XOR with state
        let mut plaintext = encrypted.ciphertext.clone();
        for (i, byte) in plaintext.iter_mut().enumerate() {
            *byte ^= self.state[i % 32];
        }

        let message =
            Message::from_vsf_bytes(&plaintext).map_err(|_| ChainError::InvalidMessage)?;

        // Advance chain state
        self.advance_state(&encrypted.ciphertext);
        self.receive_sequence += 1;

        // Process any pending messages
        self.process_pending_messages();

        Ok(message)
    }

    pub fn receive_ack(&mut self, sequence: u64) {
        // Verify this is for a message we sent and we have the ciphertext
        if let Some(ciphertext) = self.sent_messages.remove(&sequence) {
            // Advance state using the ciphertext that was successfully received
            self.advance_state(&ciphertext);
        }
    }

    fn advance_state(&mut self, ciphertext: &[u8]) {
        let mut hasher = Hasher::new();
        hasher.update(&self.state);
        hasher.update(ciphertext);
        self.state = *hasher.finalize().as_bytes();
    }

    fn process_pending_messages(&mut self) {
        while let Some(ciphertext) = self.pending_messages.remove(&self.receive_sequence) {
            // TODO: Proper decryption - for now just XOR with state
            let mut plaintext = ciphertext.clone();
            for (i, byte) in plaintext.iter_mut().enumerate() {
                *byte ^= self.state[i % 32];
            }

            match Message::from_vsf_bytes(&plaintext) {
                Ok(_message) => {
                    // Advance chain state
                    self.advance_state(&ciphertext);
                    self.receive_sequence += 1;
                    // Continue processing
                }
                Err(_) => break, // Invalid message
            }
        }
    }

    pub fn current_send_sequence(&self) -> u64 {
        self.send_sequence
    }

    pub fn current_receive_sequence(&self) -> u64 {
        self.receive_sequence
    }
}
