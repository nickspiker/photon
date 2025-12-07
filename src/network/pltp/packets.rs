//! PLTP Packet Types
//!
//! Photon Large Transfer Protocol - reliable UDP-based large transfers.
//!
//! Packet types:
//! - SPEC: VSF packet initiating transfer (total_packets, packet_size, data_hash)
//! - DATA: Minimal binary ['d', seq, ...payload] for maximum throughput
//! - ACK: VSF packet acknowledging receipt with chunk hash
//! - NAK: VSF packet requesting retransmit of missing sequences
//! - CONTROL: VSF packet for flow control (pause/resume/slow_down)
//! - COMPLETE: VSF packet with final hash verification

use crate::network::fgtw::Keypair;

/// SPEC packet - initiates a large transfer
///
/// VSF section "pltp_spec" containing:
/// - total_packets: number of DATA packets (VSF variable uint)
/// - packet_size: payload bytes per DATA packet (typically 1000)
/// - total_size: total transfer size in bytes
/// - data_hash: BLAKE3 hash of complete data for verification
/// - signature in header proves sender identity
#[derive(Clone, Debug)]
pub struct PLTPSpec {
    pub total_packets: u32,
    pub packet_size: u16,
    pub total_size: u32,
    pub data_hash: [u8; 32],
}

impl PLTPSpec {
    /// Default payload size per DATA packet
    pub const DEFAULT_PACKET_SIZE: u16 = 1000;

    /// Create SPEC for given data
    pub fn new(data: &[u8]) -> Self {
        let total_size = data.len() as u32;
        let packet_size = Self::DEFAULT_PACKET_SIZE;
        let total_packets =
            (total_size as usize + packet_size as usize - 1) / packet_size as usize;
        let data_hash = *blake3::hash(data).as_bytes();

        Self {
            total_packets: total_packets as u32,
            packet_size,
            total_size,
            data_hash,
        }
    }

    /// Serialize to VSF bytes with signature
    pub fn to_vsf_bytes(&self, keypair: &Keypair) -> Vec<u8> {
        use vsf::{VsfBuilder, VsfType};

        let provenance = self.compute_provenance();
        let sig = keypair.sign(&provenance);
        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(&sig.to_bytes());

        VsfBuilder::new()
            .provenance_hash(provenance)
            .signature_ed25519(*keypair.public.as_bytes(), sig_bytes)
            .add_section(
                "pltp_spec",
                vec![
                    ("count".to_string(), VsfType::u(self.total_packets as usize, false)),
                    ("psize".to_string(), VsfType::u(self.packet_size as usize, false)),
                    ("total".to_string(), VsfType::u(self.total_size as usize, false)),
                    ("hash".to_string(), VsfType::hb(self.data_hash.to_vec())),
                ],
            )
            .build()
            .unwrap_or_default()
    }

    /// Parse from VSF bytes
    pub fn from_vsf_fields(fields: &[(String, vsf::VsfType)]) -> Option<Self> {
        use vsf::VsfType;

        let total_packets = fields.iter().find(|(k, _)| k == "count").and_then(|(_, v)| {
            match v {
                VsfType::u(n, _) => Some(*n as u32),
                VsfType::u3(n) => Some(*n as u32),
                VsfType::u4(n) => Some(*n as u32),
                VsfType::u5(n) => Some(*n as u32),
                _ => None,
            }
        })?;

        let packet_size = fields.iter().find(|(k, _)| k == "psize").and_then(|(_, v)| {
            match v {
                VsfType::u(n, _) => Some(*n as u16),
                VsfType::u3(n) => Some(*n as u16),
                VsfType::u4(n) => Some(*n as u16),
                _ => None,
            }
        })?;

        let total_size = fields.iter().find(|(k, _)| k == "total").and_then(|(_, v)| {
            match v {
                VsfType::u(n, _) => Some(*n as u32),
                VsfType::u3(n) => Some(*n as u32),
                VsfType::u4(n) => Some(*n as u32),
                VsfType::u5(n) => Some(*n as u32),
                _ => None,
            }
        })?;

        let data_hash = fields.iter().find(|(k, _)| k == "hash").and_then(|(_, v)| {
            match v {
                VsfType::hb(bytes) if bytes.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(bytes);
                    Some(arr)
                }
                _ => None,
            }
        })?;

        Some(Self {
            total_packets,
            packet_size,
            total_size,
            data_hash,
        })
    }

    fn compute_provenance(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"PLTP_SPEC_v1");
        hasher.update(&self.total_packets.to_le_bytes());
        hasher.update(&self.packet_size.to_le_bytes());
        hasher.update(&self.total_size.to_le_bytes());
        hasher.update(&self.data_hash);
        *hasher.finalize().as_bytes()
    }

    /// Compute bytes needed for sequence number based on total_packets
    pub fn seq_bytes(&self) -> usize {
        if self.total_packets <= 127 {
            1
        } else if self.total_packets <= 16383 {
            2
        } else if self.total_packets <= 2097151 {
            3
        } else {
            4
        }
    }
}

/// DATA packet - minimal header for maximum throughput
///
/// Format: ['d', seq_vsf, ...payload]
/// - 'd' (1 byte): packet type marker
/// - seq_vsf: VSF-style variable-length sequence number
/// - payload: raw data bytes (up to packet_size from SPEC)
#[derive(Clone, Debug)]
pub struct PLTPData {
    pub sequence: u32,
    pub payload: Vec<u8>,
}

impl PLTPData {
    /// Serialize to wire format
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(1 + 4 + self.payload.len());
        bytes.push(b'd');
        bytes.extend_from_slice(&encode_vsf_uint(self.sequence));
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    /// Parse from wire format, using expected seq_bytes from SPEC
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() || bytes[0] != b'd' {
            return None;
        }

        let (sequence, seq_len) = decode_vsf_uint(&bytes[1..])?;
        let payload = bytes[1 + seq_len..].to_vec();

        Some(Self {
            sequence: sequence as u32,
            payload,
        })
    }
}

/// ACK packet - acknowledge receipt of DATA packet
///
/// VSF section "pltp_ack" containing:
/// - seq: sequence number being acknowledged
/// - chunk_hash: BLAKE3 of the received payload (proves correct receipt)
/// - buffer_pct: receiver buffer utilization 0-100 (for flow control)
#[derive(Clone, Debug)]
pub struct PLTPAck {
    pub sequence: u32,
    pub chunk_hash: [u8; 32],
    pub buffer_percent: u8,
}

impl PLTPAck {
    /// Create ACK for received data
    pub fn new(sequence: u32, payload: &[u8], buffer_percent: u8) -> Self {
        Self {
            sequence,
            chunk_hash: *blake3::hash(payload).as_bytes(),
            buffer_percent,
        }
    }

    /// Serialize to VSF bytes
    pub fn to_vsf_bytes(&self, keypair: &Keypair) -> Vec<u8> {
        use vsf::{VsfBuilder, VsfType};

        let provenance = self.compute_provenance();
        let sig = keypair.sign(&provenance);
        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(&sig.to_bytes());

        VsfBuilder::new()
            .provenance_hash(provenance)
            .signature_ed25519(*keypair.public.as_bytes(), sig_bytes)
            .add_section(
                "pltp_ack",
                vec![
                    ("seq".to_string(), VsfType::u(self.sequence as usize, false)),
                    ("hash".to_string(), VsfType::hb(self.chunk_hash.to_vec())),
                    ("buf".to_string(), VsfType::u3(self.buffer_percent)),
                ],
            )
            .build()
            .unwrap_or_default()
    }

    /// Parse from VSF fields
    pub fn from_vsf_fields(fields: &[(String, vsf::VsfType)]) -> Option<Self> {
        use vsf::VsfType;

        let sequence = fields.iter().find(|(k, _)| k == "seq").and_then(|(_, v)| {
            match v {
                VsfType::u(n, _) => Some(*n as u32),
                VsfType::u3(n) => Some(*n as u32),
                VsfType::u4(n) => Some(*n as u32),
                VsfType::u5(n) => Some(*n as u32),
                VsfType::u6(n) => Some(*n as u32),
                _ => None,
            }
        })?;

        let chunk_hash = fields.iter().find(|(k, _)| k == "hash").and_then(|(_, v)| {
            match v {
                VsfType::hb(bytes) if bytes.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(bytes);
                    Some(arr)
                }
                _ => None,
            }
        })?;

        let buffer_percent = fields
            .iter()
            .find(|(k, _)| k == "buf")
            .and_then(|(_, v)| match v {
                VsfType::u3(n) => Some(*n),
                VsfType::u(n, _) => Some(*n as u8),
                _ => None,
            })
            .unwrap_or(0);

        Some(Self {
            sequence,
            chunk_hash,
            buffer_percent,
        })
    }

    fn compute_provenance(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"PLTP_ACK_v1");
        hasher.update(&self.sequence.to_le_bytes());
        hasher.update(&self.chunk_hash);
        *hasher.finalize().as_bytes()
    }
}

/// NAK packet - request retransmission of missing sequences
///
/// VSF section "pltp_nak" containing:
/// - seqs: list of missing sequence numbers
#[derive(Clone, Debug)]
pub struct PLTPNak {
    pub missing_sequences: Vec<u32>,
}

impl PLTPNak {
    /// Serialize to VSF bytes
    pub fn to_vsf_bytes(&self, keypair: &Keypair) -> Vec<u8> {
        use vsf::{VsfBuilder, VsfType, Tensor};

        let provenance = self.compute_provenance();
        let sig = keypair.sign(&provenance);
        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(&sig.to_bytes());

        // Encode sequences as bytes (4 bytes each, little-endian)
        let seq_bytes: Vec<u8> = self
            .missing_sequences
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();

        VsfBuilder::new()
            .provenance_hash(provenance)
            .signature_ed25519(*keypair.public.as_bytes(), sig_bytes)
            .add_section(
                "pltp_nak",
                vec![(
                    "seqs".to_string(),
                    VsfType::t_u3(Tensor::new(vec![seq_bytes.len()], seq_bytes)),
                )],
            )
            .build()
            .unwrap_or_default()
    }

    /// Parse from VSF fields
    pub fn from_vsf_fields(fields: &[(String, vsf::VsfType)]) -> Option<Self> {
        use vsf::VsfType;

        let seq_bytes = fields.iter().find(|(k, _)| k == "seqs").and_then(|(_, v)| {
            match v {
                VsfType::t_u3(tensor) => Some(tensor.data.clone()),
                _ => None,
            }
        })?;

        // Decode sequences (4 bytes each)
        let missing_sequences: Vec<u32> = seq_bytes
            .chunks_exact(4)
            .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();

        Some(Self { missing_sequences })
    }

    fn compute_provenance(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"PLTP_NAK_v1");
        for seq in &self.missing_sequences {
            hasher.update(&seq.to_le_bytes());
        }
        *hasher.finalize().as_bytes()
    }
}

/// Flow control commands
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ControlCommand {
    Pause = 0,
    Resume = 1,
    SlowDown = 2,
    Abort = 3,
}

impl ControlCommand {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Pause),
            1 => Some(Self::Resume),
            2 => Some(Self::SlowDown),
            3 => Some(Self::Abort),
            _ => None,
        }
    }
}

/// CONTROL packet - flow control signals
///
/// VSF section "pltp_ctrl" containing:
/// - cmd: control command (0=pause, 1=resume, 2=slow_down, 3=abort)
#[derive(Clone, Debug)]
pub struct PLTPControl {
    pub command: ControlCommand,
}

impl PLTPControl {
    /// Serialize to VSF bytes
    pub fn to_vsf_bytes(&self, keypair: &Keypair) -> Vec<u8> {
        use vsf::{VsfBuilder, VsfType};

        let provenance = self.compute_provenance();
        let sig = keypair.sign(&provenance);
        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(&sig.to_bytes());

        VsfBuilder::new()
            .provenance_hash(provenance)
            .signature_ed25519(*keypair.public.as_bytes(), sig_bytes)
            .add_section(
                "pltp_ctrl",
                vec![("cmd".to_string(), VsfType::u3(self.command as u8))],
            )
            .build()
            .unwrap_or_default()
    }

    /// Parse from VSF fields
    pub fn from_vsf_fields(fields: &[(String, vsf::VsfType)]) -> Option<Self> {
        use vsf::VsfType;

        let cmd = fields.iter().find(|(k, _)| k == "cmd").and_then(|(_, v)| {
            match v {
                VsfType::u3(n) => ControlCommand::from_u8(*n),
                VsfType::u(n, _) => ControlCommand::from_u8(*n as u8),
                _ => None,
            }
        })?;

        Some(Self { command: cmd })
    }

    fn compute_provenance(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"PLTP_CTRL_v1");
        hasher.update(&[self.command as u8]);
        *hasher.finalize().as_bytes()
    }
}

/// COMPLETE packet - final transfer verification
///
/// VSF section "pltp_done" containing:
/// - final_hash: BLAKE3 of reassembled data
/// - success: whether hash matched expected
#[derive(Clone, Debug)]
pub struct PLTPComplete {
    pub final_hash: [u8; 32],
    pub success: bool,
}

impl PLTPComplete {
    /// Serialize to VSF bytes
    pub fn to_vsf_bytes(&self, keypair: &Keypair) -> Vec<u8> {
        use vsf::{VsfBuilder, VsfType};

        let provenance = self.compute_provenance();
        let sig = keypair.sign(&provenance);
        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(&sig.to_bytes());

        VsfBuilder::new()
            .provenance_hash(provenance)
            .signature_ed25519(*keypair.public.as_bytes(), sig_bytes)
            .add_section(
                "pltp_done",
                vec![
                    ("hash".to_string(), VsfType::hb(self.final_hash.to_vec())),
                    ("ok".to_string(), VsfType::u3(if self.success { 1 } else { 0 })),
                ],
            )
            .build()
            .unwrap_or_default()
    }

    /// Parse from VSF fields
    pub fn from_vsf_fields(fields: &[(String, vsf::VsfType)]) -> Option<Self> {
        use vsf::VsfType;

        let final_hash = fields.iter().find(|(k, _)| k == "hash").and_then(|(_, v)| {
            match v {
                VsfType::hb(bytes) if bytes.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(bytes);
                    Some(arr)
                }
                _ => None,
            }
        })?;

        let success = fields
            .iter()
            .find(|(k, _)| k == "ok")
            .map(|(_, v)| match v {
                VsfType::u3(n) => *n != 0,
                VsfType::u(n, _) => *n != 0,
                _ => false,
            })
            .unwrap_or(false);

        Some(Self { final_hash, success })
    }

    fn compute_provenance(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"PLTP_DONE_v1");
        hasher.update(&self.final_hash);
        hasher.update(&[if self.success { 1 } else { 0 }]);
        *hasher.finalize().as_bytes()
    }
}

// ============================================================================
// VSF Variable-Length Uint Encoding (matching VSF spec)
// ============================================================================

/// Encode unsigned integer as VSF variable-length format
/// - 0-127: 1 byte (high bit clear)
/// - 128+: high bit set, continue with next byte
fn encode_vsf_uint(mut value: u32) -> Vec<u8> {
    let mut bytes = Vec::new();

    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;

        if value != 0 {
            byte |= 0x80; // Set continuation bit
        }

        bytes.push(byte);

        if value == 0 {
            break;
        }
    }

    bytes
}

/// Decode VSF variable-length uint, returns (value, bytes_consumed)
fn decode_vsf_uint(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut value: usize = 0;
    let mut shift = 0;

    for (i, &byte) in bytes.iter().enumerate() {
        value |= ((byte & 0x7F) as usize) << shift;
        shift += 7;

        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }

        if shift >= 32 {
            return None; // Overflow
        }
    }

    None // Incomplete
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vsf_uint_encoding() {
        // Single byte values
        assert_eq!(encode_vsf_uint(0), vec![0]);
        assert_eq!(encode_vsf_uint(1), vec![1]);
        assert_eq!(encode_vsf_uint(127), vec![127]);

        // Two byte values
        assert_eq!(encode_vsf_uint(128), vec![0x80, 0x01]);
        assert_eq!(encode_vsf_uint(255), vec![0xFF, 0x01]);
        assert_eq!(encode_vsf_uint(16383), vec![0xFF, 0x7F]);

        // Roundtrip
        for val in [0, 1, 127, 128, 255, 256, 16383, 16384, 65535, 548] {
            let encoded = encode_vsf_uint(val);
            let (decoded, _) = decode_vsf_uint(&encoded).unwrap();
            assert_eq!(decoded as u32, val, "roundtrip failed for {}", val);
        }
    }

    #[test]
    fn test_data_packet_roundtrip() {
        let data = PLTPData {
            sequence: 42,
            payload: vec![0xAB; 1000],
        };

        let bytes = data.to_bytes();
        assert_eq!(bytes[0], b'd');

        let parsed = PLTPData::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.sequence, 42);
        assert_eq!(parsed.payload.len(), 1000);
    }

    #[test]
    fn test_data_packet_large_sequence() {
        let data = PLTPData {
            sequence: 548, // Typical for CLUTCH full offer
            payload: vec![0xCD; 100],
        };

        let bytes = data.to_bytes();
        // 'd' + 2-byte seq + payload
        assert_eq!(bytes.len(), 1 + 2 + 100);

        let parsed = PLTPData::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.sequence, 548);
    }

    #[test]
    fn test_spec_seq_bytes() {
        // Small transfer: 17 packets (KEM response) = 1 byte seq
        let spec = PLTPSpec {
            total_packets: 17,
            packet_size: 1000,
            total_size: 17000,
            data_hash: [0; 32],
        };
        assert_eq!(spec.seq_bytes(), 1);

        // Large transfer: 548 packets (CLUTCH full offer) = 2 byte seq
        let spec = PLTPSpec {
            total_packets: 548,
            packet_size: 1000,
            total_size: 548000,
            data_hash: [0; 32],
        };
        assert_eq!(spec.seq_bytes(), 2);
    }
}
