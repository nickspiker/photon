//! PT - Photon Transport
//!
//! Unified transport for ALL Photon communication.
//! Small payloads sent directly, large payloads sharded into 1KB DATA packets if needed.
//!
//! **Primary**: UDP
//! - Small payloads (≤1400 bytes): Sent directly as VSF
//! - Large payloads: Sharded with SPEC/DATA/ACK/COMPLETE flow
//! - DATA packets: [stream_id:1][seq:varint][payload]
//!
//! **Fallback**: TCP (after UDP retries exhausted)
//! - Uses VSF L field for framing (no length prefix)
//!
//! **Last resort**: Relay via FGTW
//!
//! Features:
//! - Adaptive windowing (TCP-like congestion control)
//! - Per-packet ACKs with chunk hash verification
//! - VSF-encoded control packets, minimal DATA headers
//! - Bidirectional transfers (both parties can send simultaneously)
//! - Multiple concurrent transfers per peer (keyed by stream_id)

pub mod buffer;
pub mod packets;
pub mod state;
pub mod window;

pub use buffer::{ReceiveBuffer, SendBuffer};
pub use packets::*;
pub use state::*;
pub use window::*;

use crate::network::fgtw::Keypair;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

/// Relay fallback info for when UDP+TCP both fail
#[derive(Debug, Clone)]
pub struct RelayInfo {
    pub recipient_pubkey: [u8; 32],
    pub payload: Vec<u8>,
}

/// Result from PT tick() for each packet to send
#[derive(Debug)]
pub struct TickSend {
    pub peer_addr: SocketAddr,
    pub wire_bytes: Vec<u8>,
    pub also_tcp: bool,
    pub relay: Option<RelayInfo>,
}

/// PT Manager - coordinates transfers for all peers
pub struct PTManager {
    /// Outbound transfers (we're sending) - multiple per peer allowed
    outbound: Vec<OutboundTransfer>,
    /// Inbound transfers (we're receiving) - keyed by (peer, stream_id)
    inbound: Vec<InboundTransfer>,
    /// Our keypair for signing
    keypair: Keypair,
    /// Stale timeout (no activity for this long = abort)
    stale_timeout: Duration,
    /// Next stream_id to allocate for outbound transfers (per peer would be better but this is simpler)
    next_stream_id: u8,
    /// Monotonic transfer ID counter for external tracking
    next_transfer_id: usize,
}

impl PTManager {
    /// Create new PT manager
    pub fn new(keypair: Keypair) -> Self {
        Self {
            outbound: Vec::new(),
            inbound: Vec::new(),
            keypair,
            stale_timeout: Duration::from_secs(30),
            next_stream_id: b'a',
            next_transfer_id: 0,
        }
    }

    /// Get reference to keypair (for relay fallback)
    pub fn keypair(&self) -> &Keypair {
        &self.keypair
    }

    // =========================================================================
    // Transfer Stream Management ('a'-'z')
    // =========================================================================

    /// Allocate next available stream_id ('a'-'z', wraps around)
    fn allocate_stream_id(&mut self) -> u8 {
        let id = self.next_stream_id;
        self.next_stream_id = if self.next_stream_id >= b'z' {
            b'a'
        } else {
            self.next_stream_id + 1
        };
        id
    }

    /// Max VSF size for single UDP packet (no sharding needed)
    /// 1KB threshold - VSF this size or smaller sent directly
    /// Larger VSF gets sharded into [lowercase letter][packet number][1KB DATA] packets
    pub const SINGLE_PACKET_MAX: usize = 1024;

    /// Queue data for reliable delivery to peer
    ///
    /// PT handles everything internally:
    /// - ALL payloads use SPEC/ACK flow for reliable delivery
    /// - Small payloads sent in single DATA packet
    /// - Large payloads sharded into multiple DATA packets
    /// - Retries, TCP fallback, relay fallback - all automatic via tick()
    ///
    /// Returns (bytes to send immediately, transfer_id for tracking)
    pub fn send(&mut self, peer_addr: SocketAddr, data: Vec<u8>) -> (Vec<u8>, usize) {
        self.send_with_pubkey(peer_addr, data, None)
    }

    /// Queue data for reliable delivery with recipient pubkey for relay fallback
    ///
    /// Same as send(), but stores recipient pubkey so relay can be used if UDP+TCP fail.
    /// ALL packets (even tiny ones) use PT's ACK tracking for reliability.
    /// Returns (SPEC bytes to send, transfer_id for tracking completion)
    pub fn send_with_pubkey(
        &mut self,
        peer_addr: SocketAddr,
        data: Vec<u8>,
        recipient_pubkey: Option<[u8; 32]>,
    ) -> (Vec<u8>, usize) {
        // ALL packets now use SPEC/ACK flow for reliable delivery
        // Small payloads are sent in a single DATA packet, but still ACKed
        // This ensures CLUTCH offers, messages, etc. all have delivery confirmation

        let stream_id = self.allocate_stream_id();
        let transfer_id = self.next_transfer_id;
        self.next_transfer_id += 1;

        let mut transfer = OutboundTransfer::new(peer_addr, data, stream_id, transfer_id);

        // Store pubkey for relay fallback
        if let Some(pubkey) = recipient_pubkey {
            transfer.set_recipient_pubkey(pubkey);
        }

        let spec = transfer.build_spec();
        let spec_bytes = spec.to_vsf_bytes(&self.keypair);

        // Mark SPEC as sent for retry tracking
        transfer.mark_spec_sent();

        crate::log(&format!(
            "PT: Starting outbound transfer #{} to {} ({} bytes, stream '{}', relay={})",
            transfer_id,
            peer_addr,
            transfer.send_buffer.total_size(),
            stream_id as char,
            recipient_pubkey.is_some(),
        ));

        // Push to vec - allows multiple concurrent transfers to same peer
        self.outbound.push(transfer);

        (spec_bytes, transfer_id)
    }

    /// Handle received SPEC (start receiving)
    pub fn handle_spec(&mut self, peer_addr: SocketAddr, spec: PTSpec) -> Vec<u8> {
        crate::log(&format!(
            "PT: Received SPEC from {} - stream '{}', {} packets, {} bytes, hash {}",
            peer_addr,
            spec.stream_id as char,
            spec.total_packets,
            spec.total_size,
            hex::encode(&spec.data_hash[..4])
        ));

        let stream_id = spec.stream_id;

        // Remove any existing incomplete transfer for this (peer, stream_id)
        // A new SPEC means peer has abandoned the old transfer
        self.inbound.retain(|t| {
            !(t.peer_addr == peer_addr && t.stream_id == stream_id && !t.is_complete())
        });

        let transfer = InboundTransfer::new(peer_addr, &spec);
        self.inbound.push(transfer);

        // Send SPEC ACK (ACK with seq=MAX as special marker)
        let ack = PTAck {
            stream_id,
            sequence: u32::MAX, // Special "SPEC ACK" marker
            chunk_hash: spec.data_hash,
        };
        ack.to_vsf_bytes(&self.keypair)
    }

    /// Handle received SPEC ACK (we can start sending DATA)
    /// Routes by stream_id for concurrent transfer support
    pub fn handle_spec_ack(
        &mut self,
        peer_addr: SocketAddr,
        stream_id: u8,
        data_hash: [u8; 32],
    ) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();

        // Find the transfer by peer AND stream_id
        if let Some(transfer) = self
            .outbound
            .iter_mut()
            .find(|t| t.peer_addr == peer_addr && t.stream_id == stream_id)
        {
            transfer.spec_acked = true;
            transfer.state = TransferState::Transferring;
            transfer.last_activity = Instant::now();

            crate::log(&format!(
                "PT: SPEC ACK received from {} for stream '{}', starting DATA transfer",
                peer_addr, stream_id as char
            ));

            // Send initial window of DATA packets
            for data in transfer.packets_to_send() {
                packets.push(data.to_bytes());
            }
        } else {
            crate::log(&format!(
                "PT: SPEC ACK from {} for unknown stream '{}' (hash {})",
                peer_addr,
                stream_id as char,
                hex::encode(&data_hash[..4])
            ));
        }

        packets
    }

    /// Handle received DATA packet
    /// Routes by (peer_addr, stream_id) to support concurrent transfers
    pub fn handle_data(&mut self, peer_addr: SocketAddr, data: PTData) -> Option<Vec<u8>> {
        // Find inbound transfer by peer AND stream_id
        if let Some(transfer) = self
            .inbound
            .iter_mut()
            .find(|t| t.peer_addr == peer_addr && t.stream_id == data.stream_id && !t.is_complete())
        {
            if let Some(ack) = transfer.handle_data(&data) {
                let (recv, total) = transfer.progress();
                // Log at milestones: every 50 packets (but not 0) or completion
                if recv == total || (recv > 0 && recv % 50 == 0) {
                    crate::log(&format!(
                        "PT: Received {}/{} from {} stream '{}'",
                        recv, total, peer_addr, data.stream_id as char
                    ));
                }

                return Some(ack.to_vsf_bytes(&self.keypair));
            }
        }
        None
    }

    /// Handle received ACK
    /// Routes by (peer_addr, stream_id) to support concurrent transfers
    pub fn handle_ack(&mut self, peer_addr: SocketAddr, ack: PTAck) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();

        // Check for SPEC ACK (seq = MAX)
        if ack.sequence == u32::MAX {
            return self.handle_spec_ack(peer_addr, ack.stream_id, ack.chunk_hash);
        }

        // Find outbound transfer by peer AND stream_id
        if let Some(transfer) = self.outbound.iter_mut().find(|t| {
            t.peer_addr == peer_addr
                && t.stream_id == ack.stream_id
                && t.state == TransferState::Transferring
        }) {
            transfer.handle_ack(&ack);

            // Only log progress at milestones (every 100 packets or completion)
            // Avoids spamming logs with per-ACK updates
            let (acked, total) = transfer.send_buffer.progress();
            if acked == total {
                crate::log(&format!(
                    "PT: All {}/{} ACK'd to {} stream '{}'",
                    acked, total, peer_addr, ack.stream_id as char
                ));
            } else if acked > 0 && acked % 100 == 0 {
                crate::log(&format!(
                    "PT: Progress {}/{} to {} stream '{}'",
                    acked, total, peer_addr, ack.stream_id as char
                ));
            }

            // Send more packets (pipelining phase sends packets_per_ack new packets)
            for data in transfer.packets_for_ack() {
                packets.push(data.to_bytes());
            }
        }

        packets
    }

    /// Handle received NAK
    pub fn handle_nak(&mut self, peer_addr: SocketAddr, nak: PTNak) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();

        if let Some(transfer) = self
            .outbound
            .iter_mut()
            .find(|t| t.peer_addr == peer_addr && t.state == TransferState::Transferring)
        {
            crate::log(&format!(
                "PT: NAK received from {} - retransmitting {} packets",
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
    pub fn handle_control(&mut self, peer_addr: SocketAddr, control: PTControl) {
        match control.command {
            ControlCommand::Abort => {
                crate::log(&format!("PT: Peer {} aborted transfer", peer_addr));
                self.outbound.retain(|t| t.peer_addr != peer_addr);
                self.inbound.retain(|t| t.peer_addr != peer_addr);
            }
            ControlCommand::Pause => {
                // Could pause sending, but for now just log
                crate::log(&format!("PT: Peer {} requested pause", peer_addr));
            }
            ControlCommand::Resume => {
                crate::log(&format!("PT: Peer {} requested resume", peer_addr));
            }
            ControlCommand::SlowDown => {
                if let Some(transfer) = self
                    .outbound
                    .iter_mut()
                    .find(|t| t.peer_addr == peer_addr && t.state == TransferState::Transferring)
                {
                    transfer.window.on_loss(); // Treat SlowDown like loss - backs off send ratio
                    crate::log(&format!("PT: Slowing down to {}", peer_addr));
                }
            }
        }
    }

    /// Handle received COMPLETE
    pub fn handle_complete(&mut self, peer_addr: SocketAddr, complete: PTComplete) {
        // Find transfer by peer and final_hash
        if let Some(transfer) = self
            .outbound
            .iter_mut()
            .find(|t| t.peer_addr == peer_addr && t.send_buffer.data_hash() == complete.final_hash)
        {
            let (packets, bytes, retransmits, duration_ms, max_window, rtt_ms, packet_size) =
                transfer.stats();
            transfer.handle_complete(&complete);

            if complete.success {
                // Calculate utilization metrics
                let total_sent = packets + retransmits;
                let utilization = if total_sent > 0 {
                    (packets as f64 / total_sent as f64) * 100.0
                } else {
                    100.0
                };
                let throughput_kbps = if duration_ms > 0 {
                    (bytes as f64 * 8.0) / (duration_ms as f64) // kbps
                } else {
                    0.0
                };
                let throughput_str = if throughput_kbps >= 1000.0 {
                    format!("{:.1} Mbps", throughput_kbps / 1000.0)
                } else {
                    format!("{:.0} kbps", throughput_kbps)
                };

                crate::log(&format!(
                    "PT: → {} OK | {} | {:.1}s | {}B pkt | win {} | RTT {}ms | {:.0}% util ({} retx)",
                    peer_addr,
                    throughput_str,
                    duration_ms as f64 / 1000.0,
                    packet_size,
                    max_window,
                    rtt_ms,
                    utilization,
                    retransmits,
                ));
            } else {
                crate::log(&format!(
                    "PT: → {} FAILED verification ({} packets, {} bytes)",
                    peer_addr, packets, bytes
                ));
            }
        }
    }

    /// Check if inbound transfer is complete, return COMPLETE packet to send
    pub fn check_inbound_complete(&mut self, peer_addr: SocketAddr) -> Option<Vec<u8>> {
        if let Some(transfer) = self
            .inbound
            .iter()
            .find(|t| t.peer_addr == peer_addr && t.is_complete())
        {
            let complete = transfer.build_complete();
            return Some(complete.to_vsf_bytes(&self.keypair));
        }
        None
    }

    /// Get inbound transfer stats (before taking data)
    /// Returns: (total_packets, total_bytes, duplicates, duration_ms)
    pub fn inbound_stats(&self, peer_addr: &SocketAddr) -> Option<(u32, u32, u32, u64)> {
        self.inbound
            .iter()
            .find(|t| t.peer_addr == *peer_addr)
            .map(|t| t.stats())
    }

    /// Take completed inbound data (consumes transfer)
    pub fn take_inbound_data(&mut self, peer_addr: SocketAddr) -> Option<Vec<u8>> {
        // Find and remove the completed transfer
        let idx = self.inbound.iter().position(|t| {
            t.peer_addr == peer_addr && t.is_complete() && t.receive_buffer.verify()
        })?;

        let transfer = self.inbound.remove(idx);
        Some(transfer.take_data())
    }

    /// Check if outbound transfer is complete (by peer address - any transfer)
    pub fn is_outbound_complete(&self, peer_addr: &SocketAddr) -> bool {
        self.outbound
            .iter()
            .any(|t| t.peer_addr == *peer_addr && t.state == TransferState::Complete)
    }

    /// Check if outbound transfer is complete (by transfer ID - specific transfer)
    pub fn is_outbound_complete_by_id(&self, transfer_id: usize) -> bool {
        self.outbound
            .iter()
            .any(|t| t.transfer_id == transfer_id && t.state == TransferState::Complete)
    }

    /// Remove completed outbound transfer by transfer ID
    pub fn remove_outbound_by_id(&mut self, transfer_id: usize) {
        self.outbound
            .retain(|t| !(t.transfer_id == transfer_id && t.state == TransferState::Complete));
    }

    /// Remove completed outbound transfer
    pub fn remove_outbound(&mut self, peer_addr: &SocketAddr) {
        self.outbound
            .retain(|t| !(t.peer_addr == *peer_addr && t.state == TransferState::Complete));
    }

    /// Clear ALL outbound transfers to a peer (regardless of state)
    /// Used when CLUTCH completes to stop retransmitting offers/KEM responses.
    pub fn clear_outbound(&mut self, peer_addr: &SocketAddr) {
        let before = self.outbound.len();
        self.outbound.retain(|t| t.peer_addr != *peer_addr);
        let removed = before - self.outbound.len();
        if removed > 0 {
            crate::log(&format!(
                "PT: Cleared {} outbound transfers to {} (forced)",
                removed, peer_addr
            ));
        }
    }

    /// Periodic tick - check timeouts, send retransmits
    /// Returns TickSend structs with:
    /// - peer_addr, wire_bytes: UDP packet to send
    /// - also_tcp: if true, also send via TCP
    /// - relay: if Some, UDP+TCP failed, relay via /conduit with this info
    pub fn tick(&mut self) -> Vec<TickSend> {
        let mut to_send = Vec::new();

        // Check outbound transfers
        for transfer in &mut self.outbound {
            if transfer.is_stale(self.stale_timeout) {
                crate::log(&format!(
                    "PT: Outbound transfer to {} timed out",
                    transfer.peer_addr
                ));
                transfer.state = TransferState::Failed;
                continue;
            }

            // SPEC retry with exponential backoff
            if transfer.spec_needs_retry() {
                // After 1s, also try TCP in parallel
                let tcp_eligible = transfer.tcp_eligible();
                if tcp_eligible && !transfer.spec_tcp_fallback {
                    transfer.set_spec_tcp_fallback();
                    crate::log(&format!(
                        "PT: SPEC for stream '{}' to {} - adding TCP parallel",
                        transfer.stream_id as char, transfer.peer_addr
                    ));
                }

                // Check if we should try relay (both UDP and TCP exhausted)
                let use_relay = transfer.should_relay_fallback();

                transfer.mark_spec_sent();
                let spec = transfer.build_spec();
                let spec_bytes = spec.to_vsf_bytes(&self.keypair);

                // Build relay info if needed (requires recipient pubkey and original payload)
                let relay = if use_relay {
                    match (transfer.recipient_pubkey, transfer.original_payload.as_ref()) {
                        (Some(pubkey), Some(payload)) => {
                            crate::log(&format!(
                                "PT: SPEC stream '{}' to {} - falling back to relay",
                                transfer.stream_id as char, transfer.peer_addr
                            ));
                            Some(RelayInfo {
                                recipient_pubkey: pubkey,
                                payload: payload.clone(),
                            })
                        }
                        _ => {
                            crate::log(&format!(
                                "PT: SPEC stream '{}' to {} - relay needed but no pubkey/payload",
                                transfer.stream_id as char, transfer.peer_addr
                            ));
                            None
                        }
                    }
                } else {
                    crate::log(&format!(
                        "PT: Retrying SPEC stream '{}' to {} (attempt {}, tcp={})",
                        transfer.stream_id as char,
                        transfer.peer_addr,
                        transfer.spec_retry_count,
                        transfer.spec_tcp_fallback
                    ));
                    None
                };

                to_send.push(TickSend {
                    peer_addr: transfer.peer_addr,
                    wire_bytes: spec_bytes,
                    also_tcp: transfer.spec_tcp_fallback,
                    relay,
                });
            }

            // Check for DATA packet timeouts (only during transfer phase)
            if transfer.state == TransferState::Transferring {
                let tcp_eligible = transfer.tcp_eligible();
                for data in transfer.check_timeouts() {
                    to_send.push(TickSend {
                        peer_addr: transfer.peer_addr,
                        wire_bytes: data.to_bytes(),
                        also_tcp: tcp_eligible,
                        relay: None, // DATA packets don't use relay
                    });
                }
            }
        }

        // Check inbound timeouts
        for transfer in &mut self.inbound {
            if transfer.is_stale(self.stale_timeout) {
                crate::log(&format!(
                    "PT: Inbound transfer from {} timed out",
                    transfer.peer_addr
                ));
                transfer.state = TransferState::Failed;
            }
        }

        // Remove failed transfers
        self.outbound.retain(|t| t.state != TransferState::Failed);
        self.inbound.retain(|t| t.state != TransferState::Failed);

        to_send
    }

    /// Check if we have an active transfer with peer
    pub fn has_transfer(&self, peer_addr: &SocketAddr) -> bool {
        self.outbound.iter().any(|t| t.peer_addr == *peer_addr)
            || self.inbound.iter().any(|t| t.peer_addr == *peer_addr)
    }

    /// Get outbound transfer state
    pub fn outbound_state(&self, peer_addr: &SocketAddr) -> Option<TransferState> {
        self.outbound
            .iter()
            .find(|t| t.peer_addr == *peer_addr)
            .map(|t| t.state)
    }

    /// Get inbound transfer state
    pub fn inbound_state(&self, peer_addr: &SocketAddr) -> Option<TransferState> {
        self.inbound
            .iter()
            .find(|t| t.peer_addr == *peer_addr)
            .map(|t| t.state)
    }
}

/// Check if bytes are a PT DATA packet (starts with 'a'-'z' stream_id)
pub fn is_pt_data(bytes: &[u8]) -> bool {
    !bytes.is_empty() && (b'a'..=b'z').contains(&bytes[0])
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

        let mut sender = PTManager::new(sender_keypair);
        let mut receiver = PTManager::new(receiver_keypair);

        let peer_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let data = vec![0xAB; 3000]; // 3 packets

        // Sender initiates
        let (spec_bytes, _transfer_id) = sender.send(peer_addr, data.clone());
        assert!(!spec_bytes.is_empty());

        // Parse SPEC and feed to receiver
        let spec_fields = parse_vsf_section_fields(&spec_bytes);
        let spec = PTSpec::from_vsf_fields(&spec_fields).expect("Failed to parse SPEC");
        let spec_ack = receiver.handle_spec(peer_addr, spec.clone());
        assert!(!spec_ack.is_empty());

        // Parse SPEC ACK - it's now header-only format
        let (provenance, values) =
            parse_pt_header_field(&spec_ack).expect("Failed to parse SPEC ACK header");
        let ack = PTAck::from_vsf_header(provenance, &values).expect("Failed to parse SPEC ACK");
        assert_eq!(ack.sequence, u32::MAX); // SPEC ACK marker

        let mut data_packets = sender.handle_spec_ack(peer_addr, spec.stream_id, spec.data_hash);
        assert!(!data_packets.is_empty(), "Should have data packets to send");

        // Process DATA packets - window starts at 1 so we need multiple rounds
        loop {
            let mut new_packets = Vec::new();

            for data_bytes in &data_packets {
                let data_pkt = PTData::from_bytes(data_bytes).expect("Failed to parse DATA packet");
                let ack_bytes = receiver
                    .handle_data(peer_addr, data_pkt)
                    .expect("Should get ACK for DATA");

                // Parse ACK - header-only format
                let (provenance, values) =
                    parse_pt_header_field(&ack_bytes).expect("Failed to parse DATA ACK header");
                let ack =
                    PTAck::from_vsf_header(provenance, &values).expect("Failed to parse DATA ACK");

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

        // Check completion - header-only format
        let complete_bytes = receiver
            .check_inbound_complete(peer_addr)
            .expect("Should have COMPLETE");
        let (provenance, values) =
            parse_pt_header_field(&complete_bytes).expect("Failed to parse COMPLETE header");
        let complete =
            PTComplete::from_vsf_header(provenance, &values).expect("Failed to parse COMPLETE");
        assert!(complete.success);

        sender.handle_complete(peer_addr, complete);
        assert!(sender.is_outbound_complete(&peer_addr));

        // Get received data
        let received = receiver
            .take_inbound_data(peer_addr)
            .expect("Should have received data");
        assert_eq!(received, data);
    }

    #[test]
    fn test_concurrent_transfers_same_peer() {
        // Test that multiple transfers to same peer work
        let keypair = test_keypair();
        let mut manager = PTManager::new(keypair);
        let peer_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

        // Start two transfers to same peer
        let data1 = vec![0xAA; 1000];
        let data2 = vec![0xBB; 2000];

        manager.send(peer_addr, data1);
        manager.send(peer_addr, data2);

        // Both should be tracked
        assert_eq!(manager.outbound.len(), 2);

        // Both should have the same peer_addr
        assert!(manager.outbound.iter().all(|t| t.peer_addr == peer_addr));

        // But different stream_ids (sequentially assigned 'a', 'b')
        let stream_ids: Vec<_> = manager.outbound.iter().map(|t| t.stream_id).collect();
        assert_eq!(stream_ids[0], b'a');
        assert_eq!(stream_ids[1], b'b');
        assert_ne!(stream_ids[0], stream_ids[1]);

        // And different data hashes
        let hashes: Vec<_> = manager
            .outbound
            .iter()
            .map(|t| t.send_buffer.data_hash())
            .collect();
        assert_ne!(hashes[0], hashes[1]);
    }

    // Helper to parse VSF section fields (for legacy format like pt_spec)
    fn parse_vsf_section_fields(bytes: &[u8]) -> Vec<(String, vsf::VsfType)> {
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

    // Helper to parse header-only PT packet fields
    // Returns (provenance_hash, values) for the pt_* header field
    fn parse_pt_header_field(bytes: &[u8]) -> Option<([u8; 32], Vec<vsf::VsfType>)> {
        use vsf::file_format::VsfHeader;
        use vsf::parse;

        let (header, _) = VsfHeader::decode(bytes).ok()?;

        // Extract provenance hash
        let provenance_hash = match &header.provenance_hash {
            vsf::VsfType::hp(hash) if hash.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(hash);
                arr
            }
            _ => return None,
        };

        // Find pt_* header field and extract its values
        for field in &header.fields {
            if field.name.starts_with("pt_") && field.offset_bytes == 0 && field.size_bytes == 0 {
                // Parse inline values from the raw bytes
                // Skip magic "RÅ<"
                if bytes.len() < 4 || &bytes[0..3] != "RÅ".as_bytes() || bytes[3] != b'<' {
                    return None;
                }

                let mut ptr = 4;

                // Find the inline field
                while ptr < bytes.len() && bytes[ptr] != b'>' {
                    if bytes[ptr] == b'(' {
                        ptr += 1;

                        let field_name = match parse(bytes, &mut ptr) {
                            Ok(vsf::VsfType::d(name)) => name,
                            _ => continue,
                        };

                        if field_name == field.name {
                            // Found it! Parse values after ':'
                            let mut values = Vec::new();
                            if ptr < bytes.len() && bytes[ptr] == b':' {
                                ptr += 1;
                                loop {
                                    if ptr >= bytes.len() || bytes[ptr] == b')' {
                                        break;
                                    }
                                    if let Ok(value) = parse(bytes, &mut ptr) {
                                        values.push(value);
                                    } else {
                                        break;
                                    }
                                    if ptr < bytes.len() && bytes[ptr] == b',' {
                                        ptr += 1;
                                    }
                                }
                            }
                            return Some((provenance_hash, values));
                        }

                        // Skip to end of this field
                        while ptr < bytes.len() && bytes[ptr] != b')' {
                            let _ = parse(bytes, &mut ptr);
                            if ptr < bytes.len() && bytes[ptr] == b',' {
                                ptr += 1;
                            }
                        }
                        if ptr < bytes.len() && bytes[ptr] == b')' {
                            ptr += 1;
                        }
                    } else {
                        let _ = parse(bytes, &mut ptr);
                    }
                }
            }
        }

        None
    }
}
