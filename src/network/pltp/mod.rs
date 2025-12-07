//! PLTP - Photon Large Transfer Protocol
//!
//! Transport for large data transfers (CLUTCH key exchange, etc.)
//!
//! **Primary**: Raw IP protocol 254 (fast, minimal overhead)
//! - Protocol number is the discriminator - no magic bytes needed
//! - DATA packets: [4-byte seq][payload]
//! - Control packets: VSF format (detected by VSF header magic)
//!
//! **Fallback**: TCP
//! - If raw sockets fail (permissions, network blocks it)
//! - Just write() entire payload - TCP handles reliability
//! - Length-prefixed: [4-byte len][payload]
//!
//! Features:
//! - Adaptive windowing (TCP-like congestion control) for raw254
//! - Per-packet ACKs with chunk hash verification
//! - VSF-encoded control packets, minimal DATA headers
//! - Bidirectional transfers (both parties can send simultaneously)

pub mod buffer;
pub mod packets;
pub mod state;
pub mod transport;
pub mod window;

pub use buffer::{ReceiveBuffer, SendBuffer};
pub use packets::*;
pub use state::*;
pub use transport::*;
pub use window::*;

use crate::network::fgtw::Keypair;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

/// PLTP Manager - coordinates transfers for all peers
pub struct PLTPManager {
    /// Outbound transfers (we're sending)
    outbound: HashMap<SocketAddr, OutboundTransfer>,
    /// Inbound transfers (we're receiving)
    inbound: HashMap<SocketAddr, InboundTransfer>,
    /// Our keypair for signing
    keypair: Keypair,
    /// Stale timeout (no activity for this long = abort)
    stale_timeout: Duration,
}

impl PLTPManager {
    /// Create new PLTP manager
    pub fn new(keypair: Keypair) -> Self {
        Self {
            outbound: HashMap::new(),
            inbound: HashMap::new(),
            keypair,
            stale_timeout: Duration::from_secs(30),
        }
    }

    /// Start sending data to peer
    pub fn start_send(&mut self, peer_addr: SocketAddr, data: Vec<u8>) -> Vec<u8> {
        crate::log_info(&format!(
            "PLTP: Starting outbound transfer to {} ({} bytes)",
            peer_addr,
            data.len()
        ));

        let transfer = OutboundTransfer::new(peer_addr, data);
        let spec = transfer.build_spec();
        let spec_bytes = spec.to_vsf_bytes(&self.keypair);

        self.outbound.insert(peer_addr, transfer);

        spec_bytes
    }

    /// Handle received SPEC (start receiving)
    pub fn handle_spec(&mut self, peer_addr: SocketAddr, spec: PLTPSpec) -> Vec<u8> {
        crate::log_info(&format!(
            "PLTP: Received SPEC from {} - {} packets, {} bytes",
            peer_addr, spec.total_packets, spec.total_size
        ));

        let transfer = InboundTransfer::new(peer_addr, &spec);
        self.inbound.insert(peer_addr, transfer);

        // Send SPEC ACK (just an ACK with seq=0, special marker)
        let ack = PLTPAck {
            sequence: u32::MAX, // Special "SPEC ACK" marker
            chunk_hash: spec.data_hash,
            buffer_percent: 0,
        };
        ack.to_vsf_bytes(&self.keypair)
    }

    /// Handle received SPEC ACK (we can start sending DATA)
    pub fn handle_spec_ack(&mut self, peer_addr: SocketAddr) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();

        if let Some(transfer) = self.outbound.get_mut(&peer_addr) {
            transfer.spec_acked = true;
            transfer.state = TransferState::Transferring;
            transfer.last_activity = Instant::now();

            crate::log_info(&format!(
                "PLTP: SPEC ACK received from {}, starting DATA transfer",
                peer_addr
            ));

            // Send initial window of DATA packets
            for data in transfer.packets_to_send() {
                packets.push(data.to_bytes());
            }
        }

        packets
    }

    /// Handle received DATA packet
    pub fn handle_data(&mut self, peer_addr: SocketAddr, data: PLTPData) -> Option<Vec<u8>> {
        if let Some(transfer) = self.inbound.get_mut(&peer_addr) {
            if let Some(ack) = transfer.handle_data(&data) {
                let (recv, total) = transfer.progress();
                if recv % 50 == 0 || recv == total {
                    crate::log_info(&format!(
                        "PLTP: Received {}/{} from {}",
                        recv, total, peer_addr
                    ));
                }

                return Some(ack.to_vsf_bytes(&self.keypair));
            }
        }
        None
    }

    /// Handle received ACK
    pub fn handle_ack(&mut self, peer_addr: SocketAddr, ack: PLTPAck) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();

        // Check for SPEC ACK (seq = MAX)
        if ack.sequence == u32::MAX {
            return self.handle_spec_ack(peer_addr);
        }

        if let Some(transfer) = self.outbound.get_mut(&peer_addr) {
            transfer.handle_ack(&ack);

            let (acked, total) = transfer.send_buffer.progress();
            if acked % 50 == 0 || acked == total {
                crate::log_info(&format!("PLTP: ACK'd {}/{} to {}", acked, total, peer_addr));
            }

            // Send more packets if window allows
            for data in transfer.packets_to_send() {
                packets.push(data.to_bytes());
            }
        }

        packets
    }

    /// Handle received NAK
    pub fn handle_nak(&mut self, peer_addr: SocketAddr, nak: PLTPNak) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();

        if let Some(transfer) = self.outbound.get_mut(&peer_addr) {
            crate::log_info(&format!(
                "PLTP: NAK received from {} - retransmitting {} packets",
                peer_addr,
                nak.missing_sequences.len()
            ));

            for data in transfer.handle_nak(&nak) {
                packets.push(data.to_bytes());
            }
        }

        packets
    }

    /// Handle received CONTROL
    pub fn handle_control(&mut self, peer_addr: SocketAddr, control: PLTPControl) {
        match control.command {
            ControlCommand::Abort => {
                crate::log_info(&format!("PLTP: Peer {} aborted transfer", peer_addr));
                self.outbound.remove(&peer_addr);
                self.inbound.remove(&peer_addr);
            }
            ControlCommand::Pause => {
                // Could pause sending, but for now just log
                crate::log_info(&format!("PLTP: Peer {} requested pause", peer_addr));
            }
            ControlCommand::Resume => {
                crate::log_info(&format!("PLTP: Peer {} requested resume", peer_addr));
            }
            ControlCommand::SlowDown => {
                if let Some(transfer) = self.outbound.get_mut(&peer_addr) {
                    transfer.window.on_buffer_pressure();
                    crate::log_info(&format!("PLTP: Slowing down to {}", peer_addr));
                }
            }
        }
    }

    /// Handle received COMPLETE
    pub fn handle_complete(&mut self, peer_addr: SocketAddr, complete: PLTPComplete) {
        if let Some(transfer) = self.outbound.get_mut(&peer_addr) {
            transfer.handle_complete(&complete);

            if complete.success {
                crate::log_info(&format!(
                    "PLTP: Transfer to {} completed successfully!",
                    peer_addr
                ));
            } else {
                crate::log_error(&format!(
                    "PLTP: Transfer to {} failed verification",
                    peer_addr
                ));
            }
        }
    }

    /// Check if inbound transfer is complete, return COMPLETE packet to send
    pub fn check_inbound_complete(&mut self, peer_addr: SocketAddr) -> Option<Vec<u8>> {
        if let Some(transfer) = self.inbound.get(&peer_addr) {
            if transfer.is_complete() {
                let complete = transfer.build_complete();
                return Some(complete.to_vsf_bytes(&self.keypair));
            }
        }
        None
    }

    /// Take completed inbound data (consumes transfer)
    pub fn take_inbound_data(&mut self, peer_addr: SocketAddr) -> Option<Vec<u8>> {
        if let Some(transfer) = self.inbound.get(&peer_addr) {
            if transfer.is_complete() && transfer.receive_buffer.verify() {
                let transfer = self.inbound.remove(&peer_addr)?;
                return Some(transfer.take_data());
            }
        }
        None
    }

    /// Check if outbound transfer is complete
    pub fn is_outbound_complete(&self, peer_addr: &SocketAddr) -> bool {
        self.outbound
            .get(peer_addr)
            .map(|t| t.state == TransferState::Complete)
            .unwrap_or(false)
    }

    /// Remove completed outbound transfer
    pub fn remove_outbound(&mut self, peer_addr: &SocketAddr) {
        self.outbound.remove(peer_addr);
    }

    /// Periodic tick - check timeouts, send retransmits
    pub fn tick(&mut self) -> Vec<(SocketAddr, Vec<u8>)> {
        let mut to_send = Vec::new();
        let mut to_remove = Vec::new();

        // Check outbound timeouts
        for (addr, transfer) in &mut self.outbound {
            if transfer.is_stale(self.stale_timeout) {
                crate::log_error(&format!("PLTP: Outbound transfer to {} timed out", addr));
                to_remove.push(*addr);
                continue;
            }

            // Check for packet timeouts
            for data in transfer.check_timeouts() {
                to_send.push((*addr, data.to_bytes()));
            }
        }

        // Check inbound timeouts
        for (addr, transfer) in &self.inbound {
            if transfer.is_stale(self.stale_timeout) {
                crate::log_error(&format!("PLTP: Inbound transfer from {} timed out", addr));
                to_remove.push(*addr);
            }
        }

        // Remove stale transfers
        for addr in to_remove {
            self.outbound.remove(&addr);
            self.inbound.remove(&addr);
        }

        to_send
    }

    /// Check if we have an active transfer with peer
    pub fn has_transfer(&self, peer_addr: &SocketAddr) -> bool {
        self.outbound.contains_key(peer_addr) || self.inbound.contains_key(peer_addr)
    }

    /// Get outbound transfer state
    pub fn outbound_state(&self, peer_addr: &SocketAddr) -> Option<TransferState> {
        self.outbound.get(peer_addr).map(|t| t.state)
    }

    /// Get inbound transfer state
    pub fn inbound_state(&self, peer_addr: &SocketAddr) -> Option<TransferState> {
        self.inbound.get(peer_addr).map(|t| t.state)
    }
}

/// Check if bytes are a PLTP DATA packet (starts with 'd')
pub fn is_pltp_data(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes[0] == b'd'
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn test_keypair() -> Keypair {
        let secret = SigningKey::from_bytes(&[0x42; 32]);
        let public = (&secret).into();
        Keypair { secret, public }
    }

    #[test]
    fn test_full_transfer_simulation() {
        let sender_keypair = test_keypair();
        let receiver_keypair = test_keypair();

        let mut sender = PLTPManager::new(sender_keypair);
        let mut receiver = PLTPManager::new(receiver_keypair);

        let peer_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let data = vec![0xAB; 3000]; // 3 packets

        // Sender initiates
        let spec_bytes = sender.start_send(peer_addr, data.clone());
        assert!(!spec_bytes.is_empty());

        // Parse SPEC and feed to receiver
        let spec_fields = parse_vsf_fields(&spec_bytes);
        let spec = PLTPSpec::from_vsf_fields(&spec_fields).expect("Failed to parse SPEC");
        let spec_ack = receiver.handle_spec(peer_addr, spec);
        assert!(!spec_ack.is_empty());

        // Parse SPEC ACK and feed to sender - it's a special ACK with seq=MAX
        let ack_fields = parse_vsf_fields(&spec_ack);
        let ack = PLTPAck::from_vsf_fields(&ack_fields).expect("Failed to parse SPEC ACK");
        assert_eq!(ack.sequence, u32::MAX); // SPEC ACK marker

        let mut data_packets = sender.handle_spec_ack(peer_addr);
        assert!(!data_packets.is_empty(), "Should have data packets to send");

        // Process DATA packets - window starts at 1 so we need multiple rounds
        loop {
            let mut new_packets = Vec::new();

            for data_bytes in &data_packets {
                let data_pkt =
                    PLTPData::from_bytes(data_bytes).expect("Failed to parse DATA packet");
                let ack_bytes = receiver
                    .handle_data(peer_addr, data_pkt)
                    .expect("Should get ACK for DATA");
                let ack_fields = parse_vsf_fields(&ack_bytes);
                let ack = PLTPAck::from_vsf_fields(&ack_fields).expect("Failed to parse DATA ACK");

                // handle_ack may return more packets to send as window grows
                new_packets.extend(sender.handle_ack(peer_addr, ack));
            }

            // Check if we're done
            if sender.outbound_state(&peer_addr) == Some(TransferState::AwaitingComplete) {
                break;
            }

            if new_packets.is_empty() {
                break; // No more packets to send
            }
            data_packets = new_packets;
        }

        // Check completion
        let complete_bytes = receiver
            .check_inbound_complete(peer_addr)
            .expect("Should have COMPLETE");
        let complete_fields = parse_vsf_fields(&complete_bytes);
        let complete =
            PLTPComplete::from_vsf_fields(&complete_fields).expect("Failed to parse COMPLETE");
        assert!(complete.success);

        sender.handle_complete(peer_addr, complete);
        assert!(sender.is_outbound_complete(&peer_addr));

        // Get received data
        let received = receiver
            .take_inbound_data(peer_addr)
            .expect("Should have received data");
        assert_eq!(received, data);
    }

    // Helper to parse VSF fields (simplified for tests)
    fn parse_vsf_fields(bytes: &[u8]) -> Vec<(String, vsf::VsfType)> {
        use vsf::file_format::VsfHeader;
        use vsf::parse;

        let (_, header_end) = match VsfHeader::decode(bytes) {
            Ok(h) => h,
            Err(_) => return vec![],
        };

        let mut ptr = header_end;
        if ptr >= bytes.len() || bytes[ptr] != b'[' {
            return vec![];
        }
        ptr += 1;

        // Skip section name
        let _ = parse(bytes, &mut ptr);

        let mut fields = Vec::new();
        while ptr < bytes.len() && bytes[ptr] != b']' {
            if bytes[ptr] != b'(' {
                break;
            }
            ptr += 1;

            let field_name = match parse(bytes, &mut ptr) {
                Ok(vsf::VsfType::d(name)) => name,
                _ => break,
            };

            if ptr < bytes.len() && bytes[ptr] == b':' {
                ptr += 1;
                if let Ok(value) = parse(bytes, &mut ptr) {
                    fields.push((field_name, value));
                }
            }

            if ptr < bytes.len() && bytes[ptr] == b')' {
                ptr += 1;
            }
        }

        fields
    }
}
