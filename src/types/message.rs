use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId([u8; 32]);

impl MessageId {
    pub fn new(data: [u8; 32]) -> Self {
        Self(data)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct Message {
    pub nonce: u64,
    pub sequence: u64,
    pub payload: Vec<u8>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedMessage {
    pub sequence: u64,
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageStatus {
    Pending,
    Sent,
    Delivered,
    Read,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageExpiration {
    OneHour,
    OneDay,
    OneWeek,
    OneMonth,
    Never,
}

impl MessageExpiration {
    pub fn duration_seconds(&self) -> Option<u64> {
        match self {
            Self::OneHour => Some(3600),
            Self::OneDay => Some(86400),
            Self::OneWeek => Some(604800),
            Self::OneMonth => Some(2592000),
            Self::Never => None,
        }
    }
}

impl Message {
    pub fn new(sequence: u64, payload: Vec<u8>) -> Self {
        Self {
            nonce: rand::random(),
            sequence,
            payload,
            timestamp: current_timestamp(),
        }
    }
}

pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
