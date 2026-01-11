//! PT Transfer State Machine
//!
//! Manages the lifecycle of a single transfer (send or receive).

use super::buffer::{ReceiveBuffer, SendBuffer};
use super::packets::*;
use super::window::{FlightTracker, RTTEstimator, WindowController};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

/// Transfer direction
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// We're sending data to peer
    Outbound,
    /// We're receiving data from peer
    Inbound,
}

/// Transfer state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferState {
    /// Waiting for SPEC (receiver) or SPEC_ACK (sender)
    AwaitingSpec,
    /// Transferring data packets
    Transferring,
    /// Waiting for final verification
    AwaitingComplete,
    /// Transfer completed successfully
    Complete,
    /// Transfer failed
    Failed,
}

/// Error types for PT transfers
#[derive(Clone, Debug)]
pub enum PTError {
    /// Peer timed out (no response)
    Timeout,
    /// Hash verification failed
    HashMismatch,
    /// Peer aborted transfer
    PeerAborted,
    /// Invalid packet received
    InvalidPacket(String),
    /// Too many retries
    TooManyRetries,
}

/// Outbound transfer (we're sending)
pub struct OutboundTransfer {
    pub peer_addr: SocketAddr,
    pub stream_id: u8,      // 'a'-'z' for concurrent transfer routing
    pub transfer_id: usize, // Monotonic ID for external tracking
    pub state: TransferState,
    pub send_buffer: SendBuffer,
    pub window: WindowController,
    pub rtt: RTTEstimator,
    pub flight: FlightTracker,
    pub spec_sent: bool,
    pub spec_acked: bool,
    pub complete_received: bool,
    pub retries: u32,
    pub retransmits: u32, // Count of retransmitted packets
    pub last_activity: Instant,
    pub created_at: Instant,
    /// SPEC retry tracking (exponential backoff: 1s, 2s, 4s, 8s...)
    pub spec_last_sent: Instant,
    pub spec_retry_count: u32,
    pub spec_next_delay: Duration,
    /// Whether to use TCP fallback for SPEC
    pub spec_tcp_fallback: bool,
    /// Recipient's device pubkey for relay fallback (optional)
    pub recipient_pubkey: Option<[u8; 32]>,
    /// Original payload for relay fallback (the full VSF before sharding)
    pub original_payload: Option<Vec<u8>>,
}

impl OutboundTransfer {
    /// Maximum SPEC retries before TCP fallback
    pub const SPEC_MAX_RETRIES: u32 = 5;

    /// Create new outbound transfer with assigned stream_id and transfer_id
    pub fn new(peer_addr: SocketAddr, data: Vec<u8>, stream_id: u8, transfer_id: usize) -> Self {
        // Store original payload for relay fallback (before sharding)
        let original_payload = Some(data.clone());
        Self {
            peer_addr,
            stream_id,
            transfer_id,
            state: TransferState::AwaitingSpec,
            send_buffer: SendBuffer::new(data, PTSpec::DEFAULT_PACKET_SIZE),
            window: WindowController::new(),
            rtt: RTTEstimator::new(),
            flight: FlightTracker::new(),
            spec_sent: false,
            spec_acked: false,
            complete_received: false,
            retries: 0,
            retransmits: 0,
            last_activity: Instant::now(),
            created_at: Instant::now(),
            spec_last_sent: Instant::now(),
            spec_retry_count: 0,
            spec_next_delay: Duration::from_secs(1),
            spec_tcp_fallback: false,
            recipient_pubkey: None,
            original_payload,
        }
    }

    /// Set recipient pubkey for relay fallback
    pub fn set_recipient_pubkey(&mut self, pubkey: [u8; 32]) {
        self.recipient_pubkey = Some(pubkey);
    }

    /// Check if SPEC needs retry (exponential backoff)
    pub fn spec_needs_retry(&self) -> bool {
        !self.spec_acked && self.spec_sent && self.spec_last_sent.elapsed() >= self.spec_next_delay
    }

    /// Mark SPEC as sent and update backoff
    pub fn mark_spec_sent(&mut self) {
        self.spec_sent = true;
        self.spec_last_sent = Instant::now();
        self.spec_retry_count += 1;

        // Exponential backoff: 1s → 2s → 4s → 8s → 16s → 32s (capped)
        self.spec_next_delay = std::cmp::min(
            Duration::from_secs(1 << self.spec_retry_count.min(5)),
            Duration::from_secs(32),
        );
    }

    /// Check if TCP should be used in parallel (after 1s)
    /// Returns true when transfer is old enough that TCP should be tried alongside UDP
    pub fn tcp_eligible(&self) -> bool {
        self.created_at.elapsed() >= Duration::from_secs(1)
    }

    /// Check if we should fall back to relay (both UDP and TCP exhausted)
    pub fn should_relay_fallback(&self) -> bool {
        // After 5 UDP retries + 5 TCP retries = about 62s total, try relay
        self.spec_retry_count >= Self::SPEC_MAX_RETRIES * 2 && self.spec_tcp_fallback
    }

    /// Mark SPEC as using TCP fallback (for tracking that TCP has been tried)
    pub fn set_spec_tcp_fallback(&mut self) {
        self.spec_tcp_fallback = true;
    }

    /// Build SPEC packet for this transfer
    pub fn build_spec(&self) -> PTSpec {
        PTSpec {
            stream_id: self.stream_id,
            total_packets: self.send_buffer.total_packets(),
            packet_size: self.send_buffer.packet_size(),
            total_size: self.send_buffer.total_size(),
            data_hash: self.send_buffer.data_hash(),
        }
    }

    /// Get next packets to send based on blast-256 model
    ///
    /// Phase 1 (blast): Send up to INITIAL_BLAST packets immediately
    /// Phase 2 (pipelining): Send packets_per_ack() packets for each ACK
    pub fn packets_to_send(&mut self) -> Vec<PTData> {
        let mut packets = Vec::new();

        if self.window.in_blast_phase() {
            // Blast phase: send ALL blast packets immediately (no in-flight limit)
            // We're intentionally flooding - ACKs will catch up
            while self.window.in_blast_phase() {
                if let Some(seq) = self.send_buffer.next_to_send() {
                    if let Some(payload) = self.send_buffer.get_packet(seq) {
                        packets.push(PTData {
                            stream_id: self.stream_id,
                            sequence: seq,
                            payload: payload.to_vec(),
                        });
                        self.flight.sent(seq);
                        self.window.consume_blast();
                    }
                } else {
                    // Less data than INITIAL_BLAST - exit blast phase early
                    while self.window.in_blast_phase() {
                        self.window.consume_blast();
                    }
                    break;
                }
            }
        }
        // After blast phase, packets are sent via handle_ack() using send_ratio

        packets
    }

    /// Get packets to send after receiving an ACK (pipelining phase)
    pub fn packets_for_ack(&mut self) -> Vec<PTData> {
        let mut packets = Vec::new();

        if self.window.in_blast_phase() {
            return packets; // Don't pipeline during blast
        }

        // Send packets_per_ack() new packets
        let to_send = self.window.packets_per_ack();
        for _ in 0..to_send {
            if let Some(seq) = self.send_buffer.next_to_send() {
                if let Some(payload) = self.send_buffer.get_packet(seq) {
                    packets.push(PTData {
                        stream_id: self.stream_id,
                        sequence: seq,
                        payload: payload.to_vec(),
                    });
                    self.flight.sent(seq);
                }
            } else {
                break; // No more data to send
            }
        }

        packets
    }

    /// Handle ACK received
    /// Note: chunk_hash verification is done in PTManager::handle_ack() during transfer matching
    pub fn handle_ack(&mut self, ack: &PTAck) -> bool {
        // Update RTT if we were tracking this packet
        if let Some(rtt_sample) = self.flight.acked(ack.sequence) {
            self.rtt.update(rtt_sample);
        }

        // Mark as ACK'd
        if self.send_buffer.mark_acked(ack.sequence) {
            self.window.on_ack();
            self.last_activity = Instant::now();
        }

        // Check if complete
        if self.send_buffer.is_complete() {
            self.state = TransferState::AwaitingComplete;
        }

        true
    }

    /// Handle NAK received - queue retransmits
    pub fn handle_nak(&mut self, nak: &PTNak) -> Vec<PTData> {
        self.window.on_loss();
        self.last_activity = Instant::now();

        let mut packets = Vec::new();
        for &seq in &nak.missing_sequences {
            if let Some(payload) = self.send_buffer.get_packet(seq) {
                packets.push(PTData {
                    stream_id: self.stream_id,
                    sequence: seq,
                    payload: payload.to_vec(),
                });
                self.flight.sent(seq);
                self.retransmits += 1;
            }
        }
        packets
    }

    /// Handle COMPLETE received
    pub fn handle_complete(&mut self, complete: &PTComplete) -> bool {
        self.last_activity = Instant::now();

        if complete.success && complete.final_hash == self.send_buffer.data_hash() {
            self.state = TransferState::Complete;
            self.complete_received = true;
            true
        } else {
            crate::log("PT: COMPLETE verification failed");
            self.state = TransferState::Failed;
            false
        }
    }

    /// Get transfer statistics
    /// Returns: (total_packets, bytes, retransmits, duration_ms, send_ratio_x100, rtt_ms, packet_size)
    pub fn stats(&self) -> (u32, u32, u32, u64, u32, u64, u16) {
        let duration_ms = self.created_at.elapsed().as_millis() as u64;
        let rtt_ms = self.rtt.srtt().as_millis() as u64;
        // Report send_ratio * 100 as integer (e.g., 2.0 -> 200, 1.5 -> 150)
        let send_ratio_x100 = (self.window.send_ratio() * 100.0) as u32;
        (
            self.send_buffer.total_packets(),
            self.send_buffer.total_size(),
            self.retransmits,
            duration_ms,
            send_ratio_x100,
            rtt_ms,
            self.send_buffer.packet_size(),
        )
    }

    /// Check for timed out packets
    pub fn check_timeouts(&mut self) -> Vec<PTData> {
        let timed_out = self.flight.timed_out(self.rtt.rto());

        if !timed_out.is_empty() {
            self.window.on_loss();
            self.rtt.backoff();
            self.retries += 1;
        }

        let mut packets = Vec::new();
        for seq in timed_out {
            if let Some(payload) = self.send_buffer.get_packet(seq) {
                packets.push(PTData {
                    stream_id: self.stream_id,
                    sequence: seq,
                    payload: payload.to_vec(),
                });
                self.flight.sent(seq);
            }
        }
        packets
    }

    /// Check if transfer has totally timed out
    pub fn is_stale(&self, timeout: Duration) -> bool {
        self.last_activity.elapsed() > timeout || self.retries > 10
    }
}

/// Inbound transfer (we're receiving)
pub struct InboundTransfer {
    pub peer_addr: SocketAddr,
    pub stream_id: u8, // 'a'-'z' for concurrent transfer routing
    pub state: TransferState,
    pub receive_buffer: ReceiveBuffer,
    pub duplicates: u32, // Count of duplicate packets received
    pub last_activity: Instant,
    pub created_at: Instant,
}

impl InboundTransfer {
    /// Create from received SPEC
    pub fn new(peer_addr: SocketAddr, spec: &PTSpec) -> Self {
        Self {
            peer_addr,
            stream_id: spec.stream_id,
            state: TransferState::Transferring,
            receive_buffer: ReceiveBuffer::new(
                spec.total_packets,
                spec.packet_size,
                spec.total_size,
                spec.data_hash,
            ),
            duplicates: 0,
            last_activity: Instant::now(),
            created_at: Instant::now(),
        }
    }

    /// Handle DATA packet received, returns ACK to send
    pub fn handle_data(&mut self, data: &PTData) -> Option<PTAck> {
        self.last_activity = Instant::now();

        if self.receive_buffer.insert(data.sequence, &data.payload) {
            // New packet - send ACK with stream_id for routing
            Some(PTAck::new(self.stream_id, data.sequence, &data.payload))
        } else {
            // Duplicate - track and still ACK to prevent sender retransmit
            self.duplicates += 1;
            Some(PTAck::new(self.stream_id, data.sequence, &data.payload))
        }
    }

    /// Check if transfer is complete
    pub fn is_complete(&self) -> bool {
        self.receive_buffer.is_complete()
    }

    /// Verify and build COMPLETE packet
    pub fn build_complete(&self) -> PTComplete {
        let final_hash = self.receive_buffer.compute_hash();
        let success = self.receive_buffer.verify();

        if success {
            crate::log("PT: Transfer verified successfully");
        } else {
            crate::log(&format!(
                "PT: Hash mismatch - expected {:?}, got {:?}",
                hex::encode(&self.receive_buffer.expected_hash()[..8]),
                hex::encode(&final_hash[..8])
            ));
        }

        PTComplete {
            final_hash,
            success,
        }
    }

    /// Get missing sequences for NAK
    pub fn missing_sequences(&self) -> Vec<u32> {
        self.receive_buffer.missing_sequences()
    }

    /// Take the received data (consumes buffer)
    pub fn take_data(self) -> Vec<u8> {
        self.receive_buffer.take_data()
    }

    /// Check if transfer has stalled
    pub fn is_stale(&self, timeout: Duration) -> bool {
        self.last_activity.elapsed() > timeout
    }

    /// Get progress
    pub fn progress(&self) -> (u32, u32) {
        self.receive_buffer.progress()
    }

    /// Get transfer statistics
    /// Returns: (total_packets, total_bytes, duplicates, duration_ms)
    pub fn stats(&self) -> (u32, u32, u32, u64) {
        let duration_ms = self.created_at.elapsed().as_millis() as u64;
        (
            self.receive_buffer.total_packets(),
            self.receive_buffer.total_size(),
            self.duplicates,
            duration_ms,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_outbound_transfer_basic() {
        let data = vec![0xAB; 3072]; // 3 packets of 1024 bytes
        let peer = "127.0.0.1:12345".parse().unwrap();

        let mut transfer = OutboundTransfer::new(peer, data.clone(), b'a', 0);

        assert_eq!(transfer.state, TransferState::AwaitingSpec);
        assert_eq!(transfer.stream_id, b'a');
        assert_eq!(transfer.send_buffer.total_packets(), 3);

        // Build spec
        let spec = transfer.build_spec();
        assert_eq!(spec.stream_id, b'a');
        assert_eq!(spec.total_packets, 3);
        assert_eq!(spec.packet_size, 1024);
        assert_eq!(spec.total_size, 3072);
    }

    #[test]
    fn test_inbound_transfer_basic() {
        let data = vec![0xCD; 2560]; // 3 packets (1024+1024+512)
        let hash = *blake3::hash(&data).as_bytes();
        let peer = "127.0.0.1:12345".parse().unwrap();

        let spec = PTSpec {
            stream_id: b'b',
            total_packets: 3,
            packet_size: 1024,
            total_size: 2560,
            data_hash: hash,
        };

        let mut transfer = InboundTransfer::new(peer, &spec);

        assert_eq!(transfer.state, TransferState::Transferring);
        assert_eq!(transfer.stream_id, b'b');

        // Receive packets
        let ack0 = transfer.handle_data(&PTData {
            stream_id: b'b',
            sequence: 0,
            payload: data[0..1024].to_vec(),
        });
        assert!(ack0.is_some());
        assert_eq!(ack0.unwrap().stream_id, b'b');

        let ack1 = transfer.handle_data(&PTData {
            stream_id: b'b',
            sequence: 1,
            payload: data[1024..2048].to_vec(),
        });
        assert!(ack1.is_some());

        assert!(!transfer.is_complete());

        let ack2 = transfer.handle_data(&PTData {
            stream_id: b'b',
            sequence: 2,
            payload: data[2048..2560].to_vec(),
        });
        assert!(ack2.is_some());

        assert!(transfer.is_complete());

        // Build complete
        let complete = transfer.build_complete();
        assert!(complete.success);
        assert_eq!(complete.final_hash, hash);
    }
}
