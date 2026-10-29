//! PT Transfer State Machine
//!
//! Manages the lifecycle of a single transfer (send or receive).

use super::buffer::{ReceiveBuffer, SendBuffer};
use super::packets::*;
use super::window::{FlightTracker, RTTEstimator};
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
    /// The caller's name for what this transfer carries (an attachment's content hash) — opaque to PT, reported back with progress so a bar can be drawn for THIS blob and no other (2026-09-25: progress keyed by peer address mixed every concurrent send into every bar).
    pub tag: Option<[u8; 32]>,
    /// Alternate address to race the SPEC against (e.g. LAN when `peer_addr` is WAN, or vice versa). FGTW reports both a public and a same-LAN address per device; we send the SPEC to both until one ACKs, then lock `peer_addr` to whichever path answered. `None` = single path.
    pub alt_addr: Option<SocketAddr>,
    pub stream_id: u8,      // 'a'-'z' for concurrent transfer routing
    pub transfer_id: usize, // Monotonic ID for external tracking
    pub state: TransferState,
    pub send_buffer: SendBuffer,
    pub rtt: RTTEstimator,
    pub flight: FlightTracker,
    pub spec_sent: bool,
    pub spec_acked: bool,
    pub complete_received: bool,
    pub retries: u32,
    pub retransmits: u32, // Count of retransmitted packets
    pub last_activity: Instant,
    pub created_at: Instant,
    /// SPEC retry tracking (millisecond ladder 200ms→6.4s — see mark_spec_sent for why the handshake can't self-tune on RTT)
    pub spec_last_sent: Instant,
    pub spec_retry_count: u32,
    pub spec_next_delay: Duration,
    /// Whether to use TCP fallback for SPEC
    pub spec_tcp_fallback: bool,
    /// Whether the whole-payload TCP fallback copy has already been sent. TCP is reliable + ordered and the VSF `l` field self-frames the payload, so the full VSF is sent over TCP exactly ONCE per transfer (not per shard / per retry tick — a 548 KB resend every tick would be brutal). UDP sharding stays the preferred path; this only fires after the UDP SPEC has gone unacked.
    pub tcp_sent: bool,
    /// Whether the payload has already been handed to the relay. Like `tcp_sent`, the ~548 KB CLUTCH offer must be stored on fgtw.org exactly ONCE — `should_relay_fallback` stays true on every retry tick past the threshold, so without this guard we'd re-upload the whole payload each cycle.
    pub relay_sent: bool,
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
            tag: None,
            alt_addr: None,
            stream_id,
            transfer_id,
            state: TransferState::AwaitingSpec,
            send_buffer: SendBuffer::new(data, PTSpec::DEFAULT_PACKET_SIZE),
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
            spec_next_delay: Duration::from_millis(200),
            spec_tcp_fallback: false,
            tcp_sent: false,
            relay_sent: false,
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

        // MILLISECOND ladder (Nick 2026-09-02, LAN-latency tune): 200ms → 400 → 800 → 1.6s → 3.2s (capped 6.4s), JITTERED 50–100% so peers riding out a shared outage don't sync into a retransmit storm. The SPEC handshake is the ONE PT timer that couldn't self-tune — a SPEC retry fires only while UN-acked, so no RTT sample exists yet (the SPEC-ACK would be the first), forcing a fixed ladder. The old 1s→32s floor was WAN-conservative: a dropped SPEC on a <1ms LAN waited a full second to recover. 200ms is ~10× snappier on LAN loss, still ≥1× a typical WAN RTT (a premature retry is one cheap dedup'd SPEC packet), and it pulls the relay fallback (SPEC_MAX_RETRIES) in to ~6s — exactly the earlier-is-better trigger the asymmetric-reachability peers needed. The DATA path already self-tunes on measured RTO; this brings the handshake in line.
        self.spec_next_delay = crate::jitter_dur(std::cmp::min(
            Duration::from_millis(100u64 << self.spec_retry_count.min(6)),
            Duration::from_millis(6400),
        ));
    }

    /// Check if TCP should be used in parallel (after 1s) Returns true when transfer is old enough that TCP should be tried alongside UDP
    pub fn tcp_eligible(&self) -> bool {
        self.created_at.elapsed() >= Duration::from_secs(1)
    }

    /// Check if we should fall back to relay (UDP + TCP tried, no ACK). Trigger at SPEC_MAX_RETRIES (~6s now the SPEC ladder is 200ms→3.2s; was ~31s on the 1s→16s ladder), NOT later: the old ~90s / 10-retry threshold was never reached because a re-firing CLUTCH ceremony supersedes the transfer first (field logs topped out at attempt 7), so relay NEVER engaged for the peers that needed it most (asymmetric reachability, no direct path). The faster ladder pulls this in further, which is strictly the direction that comment always wanted — the relayed copy is redundant if a direct path ACKs in the meantime, so an earlier trigger only costs one best-effort store on fgtw.org.
    pub fn should_relay_fallback(&self) -> bool {
        self.spec_retry_count >= Self::SPEC_MAX_RETRIES && self.spec_tcp_fallback
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

    /// The next never-sent DATA packet, now in flight, or None when every packet has gone out once. The manager calls it only while the path's [`super::window::PathPace`] admits one more.
    pub fn next_packet(&mut self) -> Option<PTData> {
        let seq = self.send_buffer.next_to_send()?;
        let payload = self.send_buffer.get_packet(seq)?.to_vec();
        self.flight.sent(seq);
        Some(PTData { stream_id: self.stream_id, sequence: seq, payload })
    }

    /// Handle ACK received Note: chunk_hash verification is done in PTManager::handle_ack() during transfer matching (`ack_names_chunk`)
    /// Returns the ACK's RTT sample for the path's pace: None for a packet sent more than once, or one no longer in flight.
    pub fn handle_ack(&mut self, ack: &PTAck) -> Option<Duration> {
        let sample = self.flight.acked(ack.sequence);
        if let Some(rtt_sample) = sample {
            self.rtt.update(rtt_sample);
        }

        // Mark as ACK'd
        if self.send_buffer.mark_acked(ack.sequence) {
            self.last_activity = Instant::now();
            // `retries` counts CONSECUTIVE no-progress timeout rounds, not lifetime losses — so any real progress refunds the whole stale budget. Without this, a blast into a path whose RTT hovers near the RTO (cellular: every tick finds SOME packet older than the ACK-recomputed RTO) bumps `retries` past the `is_stale` cap in under a second and kills a transfer that is actively ACKing (observed: both sides of a multi-hundred-packet offer exchange self-killed about a second after locking a working path).
            self.retries = 0;
        }

        // Check if complete
        if self.state == TransferState::Transferring && self.send_buffer.is_complete() {
            self.state = TransferState::AwaitingComplete;
            // THE COMPLETE PROBE (2026-10-21): the receiver's COMPLETE is one unacknowledged datagram, and when it was lost the transfer sat here forever, holding one of the peer's thirteen window slots.
            // The chunk that finished the set stays in flight as the probe: the DATA retransmit ladder (RTO, doubling, until `is_stale`) re-sends it, and a receiver that already answered this transfer answers the probe with its COMPLETE again (`PTManager::handle_data`).
            // Nothing else is in flight once every chunk is ACK'd, so the probe is the only thing the ladder carries.
            self.flight.clear();
            self.flight.sent(ack.sequence);
        }

        sample
    }

    /// Handle NAK received - queue retransmits (the manager halves the path's pace)
    pub fn handle_nak(&mut self, nak: &PTNak) -> Vec<PTData> {
        self.last_activity = Instant::now();

        let mut packets = Vec::new();
        for &seq in &nak.missing_sequences {
            if let Some(payload) = self.send_buffer.get_packet(seq) {
                packets.push(PTData {
                    stream_id: self.stream_id,
                    sequence: seq,
                    payload: payload.to_vec(),
                });
                self.flight.resent(seq);
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

    /// Get transfer statistics Returns: (total_packets, bytes, retransmits, duration_ms, rtt_ms, packet_size)
    pub fn stats(&self) -> (u32, u32, u32, u64, u64, u16) {
        let duration_ms = self.created_at.elapsed().as_millis() as u64;
        let rtt_ms = self.rtt.srtt().as_millis() as u64;
        (
            self.send_buffer.total_packets(),
            self.send_buffer.total_size(),
            self.retransmits,
            duration_ms,
            rtt_ms,
            self.send_buffer.packet_size(),
        )
    }

    /// Check for timed out packets: re-sent untimed, counted as retransmits (a non-empty result is a loss the manager halves the path's pace for).
    pub fn check_timeouts(&mut self) -> Vec<PTData> {
        let timed_out = self.flight.timed_out(self.rtt.rto());

        if !timed_out.is_empty() {
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
                self.flight.resent(seq);
                self.retransmits += 1;
            }
        }
        packets
    }

    /// Check if transfer has totally timed out
    pub fn is_stale(&self, timeout: Duration) -> bool {
        self.last_activity.elapsed() > timeout || self.retries > 10
    }
}

/// A small (≤1KB) reliable packet awaiting delivery acknowledgement.
///
/// Distinct from [`OutboundTransfer`] (which carries the heavy stream machinery — windows, flight tracking, reassembly): a packet is one shot of opaque bytes. PT retransmits it on exponential backoff (1s → 2s → … → 60s, indefinitely) until the receiver returns a delivery ack keyed by `packet_hash`. Per-peer the queue runs strictly one-in-flight (stop-and-wait) — `in_flight` marks the head; the rest wait their turn. Delivery only: the receiver auto-acks on byte-receipt, BEFORE the app sees the payload. Any semantic ack (MessageAck advancing the chain) is the app's separate concern layered on top.
pub struct OutboundPacket {
    pub peer_addr: SocketAddr,
    /// Alternate address raced alongside (LAN vs WAN), same as streams. `None` = single path.
    pub alt_addr: Option<SocketAddr>,
    /// The wire bytes (a complete signed VSF message ≤1KB).
    pub payload: Vec<u8>,
    /// BLAKE3(payload) — the delivery-ack key the receiver echoes.
    pub packet_hash: [u8; 32],
    /// Recipient device pubkey for relay fallback (optional), mirrors OutboundTransfer.
    pub recipient_pubkey: Option<[u8; 32]>,
    /// True once this packet is the in-flight head for its peer (being actively retransmitted).
    /// Only one packet per peer is `in_flight` at a time.
    pub in_flight: bool,
    /// When the head was last (re)transmitted; `None` until it first goes in flight.
    pub last_sent: Option<Instant>,
    /// Current backoff delay before the next retransmit (1s → 2s → … → 60s cap).
    pub next_delay: Duration,
    pub retry_count: u32,
}

impl OutboundPacket {
    /// Backoff ceiling — a peer that never answers parks here, retried forever (one in flight, so no flood).
    pub const MAX_BACKOFF: Duration = Duration::from_secs(60);

    pub fn new(
        peer_addr: SocketAddr,
        alt_addr: Option<SocketAddr>,
        payload: Vec<u8>,
        recipient_pubkey: Option<[u8; 32]>,
    ) -> Self {
        let packet_hash = *blake3::hash(&payload).as_bytes();
        Self {
            peer_addr,
            alt_addr: alt_addr.filter(|a| Some(*a) != Some(peer_addr)),
            payload,
            packet_hash,
            recipient_pubkey,
            in_flight: false,
            last_sent: None,
            next_delay: Duration::from_secs(1),
            retry_count: 0,
        }
    }

    /// Record the initial transmission of this packet (becomes the in-flight head). The first retransmit then waits `next_delay` = 1s; each retransmit doubles it via `mark_retransmit`.
    pub fn mark_sent(&mut self) {
        self.in_flight = true;
        self.last_sent = Some(Instant::now());
    }

    /// Record a retransmit and back off toward the 60s cap (2 → 4 → … → 60s), JITTERED to 50–100%.
    /// The exponential is recomputed from `retry_count` (not the previous jittered delay) so the randomness never compounds; jitter decorrelates peers retransmitting after a shared outage.
    pub fn mark_retransmit(&mut self) {
        self.last_sent = Some(Instant::now());
        self.retry_count += 1;
        let base = std::cmp::min(
            Duration::from_secs(1 << self.retry_count.min(6)),
            Self::MAX_BACKOFF,
        );
        self.next_delay = crate::jitter_dur(base);
    }

    /// True when the in-flight head's backoff has elapsed and it should be retransmitted.
    pub fn needs_retransmit(&self) -> bool {
        match self.last_sent {
            Some(t) => self.in_flight && t.elapsed() >= self.next_delay,
            None => false,
        }
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
    /// The device that signed the SPEC, when it was signed. Stream letters are each sender's own a..z, so two devices of one friend hold the same letter at once; this, not the letter, says whose transfer it is.
    pub sender: Option<[u8; 32]>,
    /// Every address that has declared this exact SPEC (same stream, same hash) or carried its DATA under a proven identity. DATA from any of them routes here without adoption guesswork.
    pub paths: Vec<SocketAddr>,
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
            sender: None,
            paths: vec![peer_addr],
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
            crate::logf!(
                "PT: Hash mismatch - expected {}, got {}",
                format!(
                    "{:?}",
                    hex::encode(&self.receive_buffer.expected_hash()[..8])
                ),
                format!("{:?}", hex::encode(&final_hash[..8]))
            );
        }

        PTComplete {
            // A failure names the hash the sender PROMISED, not the one we assembled: the sender finds its transfer by that hash, so only this lets it retire the transfer (freeing its window slot) instead of sitting in AwaitingComplete forever.
            // The assembled hash is in the log line above.
            final_hash: if success { final_hash } else { self.receive_buffer.expected_hash() },
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

    /// Get transfer statistics Returns: (total_packets, total_bytes, duplicates, duration_ms)
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

        let transfer = OutboundTransfer::new(peer, data.clone(), b'a', 0);

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
