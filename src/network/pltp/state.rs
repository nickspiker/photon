//! PLTP Transfer State Machine
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

/// Error types for PLTP transfers
#[derive(Clone, Debug)]
pub enum PLTPError {
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
    pub state: TransferState,
    pub send_buffer: SendBuffer,
    pub window: WindowController,
    pub rtt: RTTEstimator,
    pub flight: FlightTracker,
    pub spec_sent: bool,
    pub spec_acked: bool,
    pub complete_received: bool,
    pub retries: u32,
    pub last_activity: Instant,
    pub created_at: Instant,
}

impl OutboundTransfer {
    /// Create new outbound transfer
    pub fn new(peer_addr: SocketAddr, data: Vec<u8>) -> Self {
        Self {
            peer_addr,
            state: TransferState::AwaitingSpec,
            send_buffer: SendBuffer::new(data, PLTPSpec::DEFAULT_PACKET_SIZE),
            window: WindowController::new(),
            rtt: RTTEstimator::new(),
            flight: FlightTracker::new(),
            spec_sent: false,
            spec_acked: false,
            complete_received: false,
            retries: 0,
            last_activity: Instant::now(),
            created_at: Instant::now(),
        }
    }

    /// Build SPEC packet for this transfer
    pub fn build_spec(&self) -> PLTPSpec {
        PLTPSpec {
            total_packets: self.send_buffer.total_packets(),
            packet_size: self.send_buffer.packet_size(),
            total_size: self.send_buffer.total_size(),
            data_hash: self.send_buffer.data_hash(),
        }
    }

    /// Get next packets to send based on window
    pub fn packets_to_send(&mut self) -> Vec<PLTPData> {
        let mut packets = Vec::new();

        // First, send any new packets up to window
        while self.flight.can_send(self.window.window()) {
            if let Some(seq) = self.send_buffer.next_to_send() {
                if let Some(payload) = self.send_buffer.get_packet(seq) {
                    packets.push(PLTPData {
                        sequence: seq,
                        payload: payload.to_vec(),
                    });
                    self.flight.sent(seq);
                }
            } else {
                break;
            }
        }

        packets
    }

    /// Handle ACK received
    pub fn handle_ack(&mut self, ack: &PLTPAck) -> bool {
        // Verify chunk hash matches what we sent
        if let Some(payload) = self.send_buffer.get_packet(ack.sequence) {
            let expected_hash = *blake3::hash(payload).as_bytes();
            if expected_hash != ack.chunk_hash {
                crate::log_error(&format!(
                    "PLTP: ACK hash mismatch for seq {}",
                    ack.sequence
                ));
                return false;
            }
        } else {
            return false;
        }

        // Update RTT if we were tracking this packet
        if let Some(rtt_sample) = self.flight.acked(ack.sequence) {
            self.rtt.update(rtt_sample);
        }

        // Mark as ACK'd
        if self.send_buffer.mark_acked(ack.sequence) {
            self.window.on_ack();
            self.last_activity = Instant::now();

            // Check for buffer pressure
            if ack.buffer_percent > 75 {
                self.window.on_buffer_pressure();
            }
        }

        // Check if complete
        if self.send_buffer.is_complete() {
            self.state = TransferState::AwaitingComplete;
        }

        true
    }

    /// Handle NAK received - queue retransmits
    pub fn handle_nak(&mut self, nak: &PLTPNak) -> Vec<PLTPData> {
        self.window.on_loss();
        self.last_activity = Instant::now();

        let mut packets = Vec::new();
        for &seq in &nak.missing_sequences {
            if let Some(payload) = self.send_buffer.get_packet(seq) {
                packets.push(PLTPData {
                    sequence: seq,
                    payload: payload.to_vec(),
                });
                self.flight.sent(seq);
            }
        }
        packets
    }

    /// Handle COMPLETE received
    pub fn handle_complete(&mut self, complete: &PLTPComplete) -> bool {
        self.last_activity = Instant::now();

        if complete.success && complete.final_hash == self.send_buffer.data_hash() {
            self.state = TransferState::Complete;
            self.complete_received = true;
            true
        } else {
            crate::log_error("PLTP: COMPLETE verification failed");
            self.state = TransferState::Failed;
            false
        }
    }

    /// Check for timed out packets
    pub fn check_timeouts(&mut self) -> Vec<PLTPData> {
        let timed_out = self.flight.timed_out(self.rtt.rto());

        if !timed_out.is_empty() {
            self.window.on_loss();
            self.rtt.backoff();
            self.retries += 1;
        }

        let mut packets = Vec::new();
        for seq in timed_out {
            if let Some(payload) = self.send_buffer.get_packet(seq) {
                packets.push(PLTPData {
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
    pub state: TransferState,
    pub receive_buffer: ReceiveBuffer,
    pub last_activity: Instant,
    pub created_at: Instant,
}

impl InboundTransfer {
    /// Create from received SPEC
    pub fn new(peer_addr: SocketAddr, spec: &PLTPSpec) -> Self {
        Self {
            peer_addr,
            state: TransferState::Transferring,
            receive_buffer: ReceiveBuffer::new(
                spec.total_packets,
                spec.packet_size,
                spec.total_size,
                spec.data_hash,
            ),
            last_activity: Instant::now(),
            created_at: Instant::now(),
        }
    }

    /// Handle DATA packet received, returns ACK to send
    pub fn handle_data(&mut self, data: &PLTPData) -> Option<PLTPAck> {
        self.last_activity = Instant::now();

        if self.receive_buffer.insert(data.sequence, &data.payload) {
            // New packet - send ACK
            Some(PLTPAck::new(
                data.sequence,
                &data.payload,
                self.receive_buffer.buffer_percent(),
            ))
        } else {
            // Duplicate - still ACK to prevent sender retransmit
            Some(PLTPAck::new(
                data.sequence,
                &data.payload,
                self.receive_buffer.buffer_percent(),
            ))
        }
    }

    /// Check if transfer is complete
    pub fn is_complete(&self) -> bool {
        self.receive_buffer.is_complete()
    }

    /// Verify and build COMPLETE packet
    pub fn build_complete(&self) -> PLTPComplete {
        let final_hash = self.receive_buffer.compute_hash();
        let success = self.receive_buffer.verify();

        if success {
            crate::log_info("PLTP: Transfer verified successfully");
        } else {
            crate::log_error(&format!(
                "PLTP: Hash mismatch - expected {:?}, got {:?}",
                hex::encode(&self.receive_buffer.expected_hash()[..8]),
                hex::encode(&final_hash[..8])
            ));
        }

        PLTPComplete { final_hash, success }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_outbound_transfer_basic() {
        let data = vec![0xAB; 3000]; // 3 packets
        let peer = "127.0.0.1:12345".parse().unwrap();

        let mut transfer = OutboundTransfer::new(peer, data.clone());

        assert_eq!(transfer.state, TransferState::AwaitingSpec);
        assert_eq!(transfer.send_buffer.total_packets(), 3);

        // Build spec
        let spec = transfer.build_spec();
        assert_eq!(spec.total_packets, 3);
        assert_eq!(spec.packet_size, 1000);
        assert_eq!(spec.total_size, 3000);
    }

    #[test]
    fn test_inbound_transfer_basic() {
        let data = vec![0xCD; 2500]; // 3 packets
        let hash = *blake3::hash(&data).as_bytes();
        let peer = "127.0.0.1:12345".parse().unwrap();

        let spec = PLTPSpec {
            total_packets: 3,
            packet_size: 1000,
            total_size: 2500,
            data_hash: hash,
        };

        let mut transfer = InboundTransfer::new(peer, &spec);

        assert_eq!(transfer.state, TransferState::Transferring);

        // Receive packets
        let ack0 = transfer.handle_data(&PLTPData {
            sequence: 0,
            payload: data[0..1000].to_vec(),
        });
        assert!(ack0.is_some());

        let ack1 = transfer.handle_data(&PLTPData {
            sequence: 1,
            payload: data[1000..2000].to_vec(),
        });
        assert!(ack1.is_some());

        assert!(!transfer.is_complete());

        let ack2 = transfer.handle_data(&PLTPData {
            sequence: 2,
            payload: data[2000..2500].to_vec(),
        });
        assert!(ack2.is_some());

        assert!(transfer.is_complete());

        // Build complete
        let complete = transfer.build_complete();
        assert!(complete.success);
        assert_eq!(complete.final_hash, hash);
    }
}
