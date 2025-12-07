use chrono::Utc;
use vsf::{datetime_to_eagle_time, EagleTime, VsfType};

/// Message identifier (BLAKE3 hash)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageId([u8; 32]);

impl MessageId {
    pub fn new(data: [u8; 32]) -> Self {
        Self(data)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to VSF BLAKE3 hash type
    pub fn to_vsf(&self) -> VsfType {
        VsfType::hb(self.0.to_vec())
    }

    /// Create from VSF BLAKE3 hash type
    pub fn from_vsf(vsf: VsfType) -> Option<Self> {
        match vsf {
            VsfType::hb(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                Some(Self(arr))
            }
            _ => None,
        }
    }
}

/// Core message structure with VSF serialization
#[derive(Debug, Clone)]
pub struct Message {
    pub nonce: u64,
    pub sequence: u64,
    pub payload: Vec<u8>,
    pub timestamp: EagleTime,
}

impl Message {
    /// Serialize message to bare VSF bytes (lean, no headers)
    pub fn to_vsf_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(VsfType::u(self.nonce as usize, false).flatten());
        bytes.extend(VsfType::u(self.sequence as usize, false).flatten());
        bytes.extend(
            VsfType::t_u3(vsf::Tensor::new(
                vec![self.payload.len()],
                self.payload.clone(),
            ))
            .flatten(),
        );
        bytes.extend(VsfType::e(self.timestamp.et_type().clone()).flatten());
        bytes
    }

    /// Deserialize message from bare VSF bytes
    pub fn from_vsf_bytes(bytes: &[u8]) -> Result<Self, String> {
        use vsf::parse;

        let mut ptr = 0;

        // Parse fields in order
        let nonce = match parse(bytes, &mut ptr).map_err(|e| format!("Parse nonce error: {}", e))? {
            VsfType::u(v, _) => v as u64,
            VsfType::u3(v) => v as u64,
            VsfType::u4(v) => v as u64,
            VsfType::u5(v) => v as u64,
            VsfType::u6(v) => v,
            _ => return Err("Invalid nonce type".to_string()),
        };

        let sequence =
            match parse(bytes, &mut ptr).map_err(|e| format!("Parse sequence error: {}", e))? {
                VsfType::u(v, _) => v as u64,
                VsfType::u3(v) => v as u64,
                VsfType::u4(v) => v as u64,
                VsfType::u5(v) => v as u64,
                VsfType::u6(v) => v,
                _ => return Err("Invalid sequence type".to_string()),
            };

        let payload =
            match parse(bytes, &mut ptr).map_err(|e| format!("Parse payload error: {}", e))? {
                VsfType::t_u3(tensor) => tensor.data,
                _ => return Err("Invalid payload type".to_string()),
            };

        let timestamp =
            match parse(bytes, &mut ptr).map_err(|e| format!("Parse timestamp error: {}", e))? {
                VsfType::e(et) => EagleTime::new(et),
                _ => return Err("Invalid timestamp type".to_string()),
            };

        Ok(Message {
            nonce,
            sequence,
            payload,
            timestamp,
        })
    }
}

/// Encrypted message with VSF serialization
///
/// Wire format:
/// - sequence: u64 (message sequence number)
/// - ciphertext: Vec<u8> (ChaCha20-Poly1305 output)
#[derive(Debug, Clone)]
pub struct EncryptedMessage {
    pub sequence: u64,
    pub ciphertext: Vec<u8>,
}

impl EncryptedMessage {
    pub fn to_vsf_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(VsfType::u(self.sequence as usize, false).flatten());
        bytes.extend(
            VsfType::t_u3(vsf::Tensor::new(
                vec![self.ciphertext.len()],
                self.ciphertext.clone(),
            ))
            .flatten(),
        );
        bytes
    }

    pub fn from_vsf_bytes(bytes: &[u8]) -> Result<Self, String> {
        use vsf::parse;

        let mut ptr = 0;

        let sequence =
            match parse(bytes, &mut ptr).map_err(|e| format!("Parse sequence error: {}", e))? {
                VsfType::u(v, _) => v as u64,
                VsfType::u3(v) => v as u64,
                VsfType::u4(v) => v as u64,
                VsfType::u5(v) => v as u64,
                VsfType::u6(v) => v,
                _ => return Err("Invalid sequence type".to_string()),
            };

        let ciphertext =
            match parse(bytes, &mut ptr).map_err(|e| format!("Parse ciphertext error: {}", e))? {
                VsfType::t_u3(tensor) => tensor.data,
                _ => return Err("Invalid ciphertext type".to_string()),
            };

        Ok(EncryptedMessage {
            sequence,
            ciphertext,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageStatus {
    Pending,
    Sent,
    Delivered,
    Read,
    Failed,
}

impl MessageStatus {
    pub fn to_vsf(&self) -> VsfType {
        VsfType::u3(*self as u8)
    }

    pub fn from_vsf(vsf: VsfType) -> Option<Self> {
        match vsf {
            VsfType::u3(0) => Some(Self::Pending),
            VsfType::u3(1) => Some(Self::Sent),
            VsfType::u3(2) => Some(Self::Delivered),
            VsfType::u3(3) => Some(Self::Read),
            VsfType::u3(4) => Some(Self::Failed),
            _ => None,
        }
    }
}

impl Message {
    pub fn new(sequence: u64, payload: Vec<u8>) -> Self {
        Self {
            nonce: rand::random(),
            sequence,
            payload,
            timestamp: datetime_to_eagle_time(Utc::now()),
        }
    }
}
