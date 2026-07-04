//! PT - Photon Transport
//!
//! Unified transport for ALL Photon communication. Small payloads sent directly, large payloads sharded into 1KB DATA packets if needed.
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

/// Canonical form of a socket address for matching, collapsing an IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) to its plain IPv4 form. The OS hands the same peer back in different representations on different code paths — a transfer started to `<lan-ip>:4383` gets its SPEC-ACK back from `[::ffff:<lan-ip>]:4383`, and a raw `SocketAddr ==` treats those as different peers, so the ACK lands as "unknown stream" and the transfer never starts. Compare canonical forms everywhere a packet is routed to its transfer so the LAN/WAN race + lock-on actually works regardless of which representation the OS reports.
fn canon_addr(a: SocketAddr) -> SocketAddr {
    match a.ip() {
        std::net::IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => SocketAddr::new(std::net::IpAddr::V4(v4), a.port()),
            None => a,
        },
        std::net::IpAddr::V4(_) => a,
    }
}

/// True if two socket addresses name the same peer, treating IPv4 and its IPv4-mapped IPv6 form as equal.
fn same_addr(a: SocketAddr, b: SocketAddr) -> bool {
    canon_addr(a) == canon_addr(b)
}

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
    /// When `Some`, also send this WHOLE VSF payload over a TCP connection (the reliable fallback).
    /// UDP is preferred and carries `wire_bytes` (a PT shard); TCP is tried in parallel only after the UDP SPEC has gone ~1s without an ACK, and carries the entire pre-sharded VSF once — no PT stream framing, since TCP is ordered/reliable and the VSF `l` field self-frames the length.
    pub tcp_payload: Option<Vec<u8>>,
    pub relay: Option<RelayInfo>,
}

/// PT Manager - coordinates transfers for all peers
pub struct PTManager {
    /// Outbound transfers (we're sending) - multiple per peer allowed
    outbound: Vec<OutboundTransfer>,
    /// Inbound transfers (we're receiving) - keyed by (peer, stream_id)
    inbound: Vec<InboundTransfer>,
    /// Reliable small (≤1KB) packets awaiting delivery ack, in FIFO order. Per peer, only the front packet is in flight (stop-and-wait): it retransmits on 1→2→…→60s backoff until the receiver's delivery ack arrives, then it's popped and the next packet for that peer sends.
    /// Strict ordering per peer; head-of-line blocking is intentional (a stuck packet means the peer isn't answering, so nothing else would get through either).
    outbound_packets: Vec<OutboundPacket>,
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
            outbound_packets: Vec::new(),
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
    // Transfer Stream Management ('a'-'z') =========================================================================

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

    /// Max VSF size for single UDP packet (no sharding needed) 1KB threshold - VSF this size or smaller sent directly Larger VSF gets sharded into [lowercase letter][packet number][1KB DATA] packets
    pub const SINGLE_PACKET_MAX: usize = 1024;
    /// Retry cap for a reliable small packet before the stop-and-wait head is dropped and the per-peer FIFO advances. With 1→2→…→60s backoff, ~5 retries ≈ 30-60s of trying — long enough to ride out a brief blip, short enough that an undeliverable head (dead avatar request) can't blackhole the chat queued behind it. The higher layer re-queues (chat retransmit / avatar→FGTW), so a drop is a deferral, not a loss.
    pub const MAX_PACKET_RETRIES: u32 = 5;

    /// Queue data for reliable delivery to peer
    ///
    /// PT handles everything internally:
    /// - Small payloads: Sent directly
    /// - Large payloads: Sharded with SPEC/DATA/ACK/COMPLETE flow
    /// - Retries, TCP fallback, relay fallback - all automatic via tick()
    ///
    /// Returns bytes to send immediately (the payload itself or SPEC for large transfers)
    pub fn send(&mut self, peer_addr: SocketAddr, data: Vec<u8>) -> Vec<u8> {
        self.send_with_pubkey(peer_addr, data, None)
    }

    /// Queue data for reliable delivery with recipient pubkey for relay fallback
    ///
    /// Same as send(), but stores recipient pubkey so relay can be used if UDP+TCP fail.
    pub fn send_with_pubkey(
        &mut self,
        peer_addr: SocketAddr,
        data: Vec<u8>,
        recipient_pubkey: Option<[u8; 32]>,
    ) -> Vec<u8> {
        self.send_with_pubkey_and_alt(peer_addr, None, data, recipient_pubkey)
    }

    /// Same as [`send_with_pubkey`](Self::send_with_pubkey), but races the SPEC against an alternate address. FGTW reports both a public (WAN) and a same-LAN address per device; when the primary path is unreachable (e.g. WAN IPv6 returns "No route to host" between two peers behind the same router), the SPEC retries hit `alt_addr` too and the transfer locks onto whichever path ACKs first. Caller should pass the LAN address as `peer_addr` (preferred) and the WAN address as `alt_addr`.
    pub fn send_with_pubkey_and_alt(
        &mut self,
        peer_addr: SocketAddr,
        alt_addr: Option<SocketAddr>,
        data: Vec<u8>,
        recipient_pubkey: Option<[u8; 32]>,
    ) -> Vec<u8> {
        // Small payload — enqueue as a reliable packet (stop-and-wait, one in flight per peer, retransmitted on backoff in tick() until the receiver's delivery ack arrives). Returns the bytes to send NOW only if no packet is already in flight to this peer; otherwise it queues behind the in-flight head and goes out when that head is acked.
        if data.len() <= Self::SINGLE_PACKET_MAX {
            let peer_busy = self
                .outbound_packets
                .iter()
                .any(|p| same_addr(p.peer_addr, peer_addr) && p.in_flight);
            let mut pkt = OutboundPacket::new(peer_addr, alt_addr, data, recipient_pubkey);
            if peer_busy {
                // Queue behind the in-flight head; nothing to send right now.
                self.outbound_packets.push(pkt);
                return Vec::new();
            } else {
                pkt.mark_sent();
                let bytes = pkt.payload.clone();
                self.outbound_packets.push(pkt);
                return bytes;
            }
        }

        // Large payload - full SPEC/DATA/ACK/COMPLETE flow
        let stream_id = self.allocate_stream_id();
        let transfer_id = self.next_transfer_id;
        self.next_transfer_id += 1;

        let mut transfer = OutboundTransfer::new(peer_addr, data, stream_id, transfer_id);
        // Don't race against the same address twice (caller may pass equal LAN/WAN).
        transfer.alt_addr = alt_addr.filter(|a| *a != peer_addr);

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

        spec_bytes
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

        // Remove any existing incomplete transfer for this (peer, stream_id) A new SPEC means peer has abandoned the old transfer
        self.inbound.retain(|t| {
            !(same_addr(t.peer_addr, peer_addr) && t.stream_id == stream_id && !t.is_complete())
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

    /// Handle received SPEC ACK (we can start sending DATA) Routes by stream_id for concurrent transfer support
    pub fn handle_spec_ack(
        &mut self,
        peer_addr: SocketAddr,
        stream_id: u8,
        data_hash: [u8; 32],
    ) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();

        // Find the transfer by stream_id, accepting the ACK from either the primary path or the raced alternate (LAN vs WAN). Whichever address answered is the reachable one, so lock the transfer onto it and drop the alternate — DATA/ACK route by (peer_addr, stream_id), so all subsequent packets must use the path that ACKed.
        if let Some(transfer) = self.outbound.iter_mut().find(|t| {
            t.stream_id == stream_id && (same_addr(t.peer_addr, peer_addr) || t.alt_addr.map_or(false, |a| same_addr(a, peer_addr)))
        }) {
            if !same_addr(transfer.peer_addr, peer_addr) {
                crate::log(&format!(
                    "PT: SPEC ACK arrived on alternate path {} (was {}) for stream '{}' - locking onto it",
                    peer_addr, transfer.peer_addr, stream_id as char
                ));
                transfer.peer_addr = peer_addr;
            }
            transfer.alt_addr = None;
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

    /// Sentinel stream_id marking a PTAck as a small-PACKET delivery ack (not a stream ack). Real stream ids are 'a'-'z' (0x61-0x7A); 0 is unused there, so handle_ack can route on it.
    pub const PACKET_ACK_STREAM_ID: u8 = 0;

    /// Receiver side: a small reliable packet arrived — return the delivery-ack bytes to send back immediately, keyed by BLAKE3(payload). Pure transport ack (bytes received); the application still processes the payload separately and may send its own semantic ack (e.g. MessageAck).
    /// Idempotent: a duplicate packet (its ack was lost) just gets re-acked.
    pub fn build_packet_ack(&self, payload: &[u8]) -> Vec<u8> {
        let packet_hash = *blake3::hash(payload).as_bytes();
        let ack = PTAck {
            stream_id: Self::PACKET_ACK_STREAM_ID,
            sequence: 0,
            chunk_hash: packet_hash,
        };
        ack.to_vsf_bytes(&self.keypair)
    }

    /// Sender side: a packet delivery-ack arrived. Mark the matching in-flight packet delivered, drop it, and return the next queued packet's bytes for that peer to send now (stop-and-wait advance). Empty if no match or nothing queued.
    pub fn handle_packet_ack(&mut self, _ack_src: SocketAddr, packet_hash: [u8; 32]) -> Vec<u8> {
        // Match by packet_hash ALONE — never by source address. The ack comes back from whatever address the receiver saw us on (IPv4-mapped `::ffff:` form, or the LAN vs WAN we raced), which routinely differs from the peer_addr we queued under. packet_hash = BLAKE3(payload) is globally unique, so it identifies the packet unambiguously. (Matching on peer_addr too was the bug: acks never matched, packets retransmitted forever and blocked the queue.)
        let Some(pos) = self
            .outbound_packets
            .iter()
            .position(|p| p.packet_hash == packet_hash)
        else {
            return Vec::new(); // no match (already acked / unknown)
        };
        // Remember which peer this packet was for, so we promote that peer's next queued packet.
        let peer_addr = self.outbound_packets[pos].peer_addr;
        self.outbound_packets.remove(pos);

        // If nothing else is in flight to this peer, promote the next queued packet for it.
        let peer_busy = self
            .outbound_packets
            .iter()
            .any(|p| same_addr(p.peer_addr, peer_addr) && p.in_flight);
        if peer_busy {
            return Vec::new();
        }
        if let Some(next) = self
            .outbound_packets
            .iter_mut()
            .find(|p| same_addr(p.peer_addr, peer_addr) && !p.in_flight)
        {
            next.mark_sent();
            return next.payload.clone();
        }
        Vec::new()
    }

    /// Handle received DATA packet Routes by (peer_addr, stream_id) to support concurrent transfers
    pub fn handle_data(&mut self, peer_addr: SocketAddr, data: PTData) -> Option<Vec<u8>> {
        // Find inbound transfer by peer AND stream_id
        if let Some(transfer) = self
            .inbound
            .iter_mut()
            .find(|t| same_addr(t.peer_addr, peer_addr) && t.stream_id == data.stream_id && !t.is_complete())
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

    /// Handle received ACK Routes by (peer_addr, stream_id) to support concurrent transfers
    pub fn handle_ack(&mut self, peer_addr: SocketAddr, ack: PTAck) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();

        // Small-packet delivery ack (sentinel stream_id) — advance the per-peer stop-and-wait queue.
        if ack.stream_id == Self::PACKET_ACK_STREAM_ID {
            let next = self.handle_packet_ack(peer_addr, ack.chunk_hash);
            if !next.is_empty() {
                packets.push(next);
            }
            return packets;
        }

        // Check for SPEC ACK (seq = MAX)
        if ack.sequence == u32::MAX {
            return self.handle_spec_ack(peer_addr, ack.stream_id, ack.chunk_hash);
        }

        // Find outbound transfer by peer AND stream_id
        if let Some(transfer) = self.outbound.iter_mut().find(|t| {
            same_addr(t.peer_addr, peer_addr)
                && t.stream_id == ack.stream_id
                && t.state == TransferState::Transferring
        }) {
            transfer.handle_ack(&ack);

            // Only log progress at milestones (every 100 packets or completion) Avoids spamming logs with per-ACK updates
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
            .find(|t| same_addr(t.peer_addr, peer_addr) && t.state == TransferState::Transferring)
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
                self.outbound.retain(|t| !same_addr(t.peer_addr, peer_addr));
                self.inbound.retain(|t| !same_addr(t.peer_addr, peer_addr));
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
                    .find(|t| same_addr(t.peer_addr, peer_addr) && t.state == TransferState::Transferring)
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
            .find(|t| same_addr(t.peer_addr, peer_addr) && t.send_buffer.data_hash() == complete.final_hash)
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
                let thruput_kbps = if duration_ms > 0 {
                    (bytes as f64 * 8.0) / (duration_ms as f64) // kbps
                } else {
                    0.0
                };
                let thruput_str = if thruput_kbps >= 1000.0 {
                    format!("{:.1} Mbps", thruput_kbps / 1000.0)
                } else {
                    format!("{:.0} kbps", thruput_kbps)
                };

                crate::log(&format!(
                    "PT: → {} OK | {} | {:.1}s | {}B pkt | win {} | RTT {}ms | {:.0}% util ({} retx)",
                    peer_addr,
                    thruput_str,
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

    /// Check if a SPECIFIC inbound transfer (peer + stream) is complete, return its COMPLETE packet.
    /// Stream-scoped: a peer can have several concurrent transfers (e.g. a CLUTCH offer AND a KEM response in flight at once), and they must not be confused — matching by address alone grabs whichever happens to be first in the vec, which silently drops the other.
    pub fn check_inbound_complete(&mut self, peer_addr: SocketAddr, stream_id: u8) -> Option<Vec<u8>> {
        if let Some(transfer) = self.inbound.iter().find(|t| {
            same_addr(t.peer_addr, peer_addr) && t.stream_id == stream_id && t.is_complete()
        }) {
            let complete = transfer.build_complete();
            return Some(complete.to_vsf_bytes(&self.keypair));
        }
        None
    }

    /// Get inbound transfer stats (before taking data) Returns: (total_packets, total_bytes, duplicates, duration_ms)
    pub fn inbound_stats(&self, peer_addr: &SocketAddr) -> Option<(u32, u32, u32, u64)> {
        self.inbound
            .iter()
            .find(|t| t.peer_addr == *peer_addr)
            .map(|t| t.stats())
    }

    /// Take a SPECIFIC completed inbound transfer's data (consumes it). Stream-scoped — see `check_inbound_complete`: draining by peer alone confuses concurrent transfers from the same peer (e.g. a CLUTCH offer + KEM response), dropping one and deadlocking the ceremony.
    pub fn take_inbound_data(&mut self, peer_addr: SocketAddr, stream_id: u8) -> Option<Vec<u8>> {
        let idx = self.inbound.iter().position(|t| {
            same_addr(t.peer_addr, peer_addr)
                && t.stream_id == stream_id
                && t.is_complete()
                && t.receive_buffer.verify()
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

    /// Clear ALL outbound transfers to a peer (regardless of state) Used when CLUTCH completes to stop retransmitting offers/KEM responses.
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

    /// Periodic tick - check timeouts, send retransmits Returns TickSend structs with:
    /// - peer_addr, wire_bytes: UDP packet to send (the preferred path)
    /// - tcp_payload: if Some, also send this whole VSF over TCP (reliable fallback, once per transfer)
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
                // After 1s, also try TCP in parallel — but send the WHOLE VSF over TCP exactly
                // once (not the SPEC shard, and not every retry). TCP is the reliable fallback;
                // UDP sharding stays preferred and keeps going in parallel until one path ACKs.
                let tcp_eligible = transfer.tcp_eligible();
                let tcp_payload = if tcp_eligible && !transfer.tcp_sent {
                    transfer.set_spec_tcp_fallback();
                    transfer.tcp_sent = true;
                    crate::log(&format!(
                        "PT: SPEC for stream '{}' to {} - sending whole payload over TCP (fallback, once)",
                        transfer.stream_id as char, transfer.peer_addr
                    ));
                    transfer.original_payload.clone()
                } else {
                    None
                };

                // Check if we should try relay (both UDP and TCP exhausted)
                let use_relay = transfer.should_relay_fallback();

                transfer.mark_spec_sent();
                let spec = transfer.build_spec();
                let spec_bytes = spec.to_vsf_bytes(&self.keypair);

                // Build relay info if needed (requires recipient pubkey and original payload)
                let relay = if use_relay {
                    match (
                        transfer.recipient_pubkey,
                        transfer.original_payload.as_ref(),
                    ) {
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
                    wire_bytes: spec_bytes.clone(),
                    tcp_payload,
                    relay,
                });

                // Race the SPEC against the alternate path (LAN vs WAN) until one ACKs. Relay and the whole-payload TCP fallback are intentionally not duplicated here — they're attached to the primary path only (one TCP connection; 548 KB to both LAN and WAN would be wasteful).
                if let Some(alt) = transfer.alt_addr {
                    to_send.push(TickSend {
                        peer_addr: alt,
                        wire_bytes: spec_bytes,
                        tcp_payload: None,
                        relay: None,
                    });
                }
            }

            // Check for DATA packet timeouts (only during transfer phase). DATA retransmits are a UDP concern — the whole payload already went over TCP once (if eligible) during the SPEC phase, so no per-DATA TCP send here.
            if transfer.state == TransferState::Transferring {
                for data in transfer.check_timeouts() {
                    to_send.push(TickSend {
                        peer_addr: transfer.peer_addr,
                        wire_bytes: data.to_bytes(),
                        tcp_payload: None,
                        relay: None, // DATA packets don't use relay
                    });
                }
            }
        }

        // Give-up-and-advance: an in-flight head that has retried past the cap is undeliverable (dead address, peer gone) and must NOT hold the per-peer FIFO forever — otherwise one stuck packet (e.g. an AVATAR_REQUEST to a peer that won't ack) head-of-line-blocks every chat message queued behind it, which is exactly the "messages stick after CLUTCH" bug. Dropping is safe: small packets carry their own higher-layer retransmit (chat re-queues via the CHAT retransmit sweep; avatar falls back to FGTW), so a dropped PT packet just re-enters later — but the queue keeps flowing meanwhile.
        let mut advanced_peers: Vec<SocketAddr> = Vec::new();
        self.outbound_packets.retain(|pkt| {
            if pkt.in_flight && pkt.retry_count >= Self::MAX_PACKET_RETRIES {
                crate::log(&format!(
                    "PT: giving up on undeliverable packet to {} after {} retries — advancing the queue",
                    pkt.peer_addr, pkt.retry_count
                ));
                advanced_peers.push(pkt.peer_addr);
                false
            } else {
                true
            }
        });
        // Promote the next queued packet for each peer whose stuck head we just dropped (mirrors handle_packet_ack's stop-and-wait advance).
        for peer in advanced_peers {
            let peer_busy = self
                .outbound_packets
                .iter()
                .any(|p| same_addr(p.peer_addr, peer) && p.in_flight);
            if peer_busy {
                continue;
            }
            if let Some(next) = self
                .outbound_packets
                .iter_mut()
                .find(|p| same_addr(p.peer_addr, peer) && !p.in_flight)
            {
                next.mark_sent();
                let (paddr, payload, alt) = (next.peer_addr, next.payload.clone(), next.alt_addr);
                to_send.push(TickSend { peer_addr: paddr, wire_bytes: payload.clone(), tcp_payload: None, relay: None });
                if let Some(alt) = alt {
                    to_send.push(TickSend { peer_addr: alt, wire_bytes: payload, tcp_payload: None, relay: None });
                }
            }
        }

        // Retransmit reliable small packets whose backoff has elapsed (stop-and-wait per peer: only in-flight heads retransmit; queued packets wait for their head to be acked). Raced LAN/WAN like streams. No TCP/relay here — a 60s-capped UDP retry is the reliability for small packets; if a peer is truly unreachable, nothing flows anyway.
        for pkt in self.outbound_packets.iter_mut() {
            if pkt.in_flight && pkt.needs_retransmit() {
                pkt.mark_retransmit();
                crate::log(&format!(
                    "PT: Retransmitting packet to {} (attempt {}, next backoff {}s)",
                    pkt.peer_addr,
                    pkt.retry_count,
                    pkt.next_delay.as_secs()
                ));
                to_send.push(TickSend {
                    peer_addr: pkt.peer_addr,
                    wire_bytes: pkt.payload.clone(),
                    tcp_payload: None,
                    relay: None,
                });
                if let Some(alt) = pkt.alt_addr {
                    to_send.push(TickSend {
                        peer_addr: alt,
                        wire_bytes: pkt.payload.clone(),
                        tcp_payload: None,
                        relay: None,
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
        let spec_bytes = sender.send(peer_addr, data.clone());
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
            .check_inbound_complete(peer_addr, b'a')
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
            .take_inbound_data(peer_addr, b'a')
            .expect("Should have received data");
        assert_eq!(received, data);
    }

    #[test]
    fn test_concurrent_transfers_same_peer() {
        // Test that multiple transfers to same peer work
        let keypair = test_keypair();
        let mut manager = PTManager::new(keypair);
        let peer_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

        // Start two transfers to same peer. BOTH must exceed SINGLE_PACKET_MAX (1024) so they take the multi-packet `outbound` spec path this test asserts on — a payload ≤ 1024 takes the small-packet fast path into `outbound_packets` (a separate queue) and would never appear in `outbound`. (data1 was 1000 here, below the threshold, which silently broke this test when the small-packet path was added.)
        let data1 = vec![0xAA; 1500];
        let data2 = vec![0xBB; 2000];

        manager.send(peer_addr, data1);
        manager.send(peer_addr, data2);

        // Both should be tracked
        assert_eq!(manager.outbound.len(), 2);

        // Both should have the same peer_addr
        assert!(manager.outbound.iter().all(|t| same_addr(t.peer_addr, peer_addr)));

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

    #[test]
    fn test_concurrent_inbound_drains_correct_stream() {
        // The CLUTCH deadlock: two transfers from the SAME peer in flight at once (an offer + a KEM response). The completion check + drain must be stream-scoped, or one is silently dropped.
        let keypair = test_keypair();
        let mut mgr = PTManager::new(keypair);
        let peer: SocketAddr = "127.0.0.1:12345".parse().unwrap();

        // Two single-packet inbound transfers, distinct streams + distinct payloads.
        let data_a = vec![0xAA; 64];
        let data_b = vec![0xBB; 64];
        let spec = |sid: u8, d: &[u8]| PTSpec {
            stream_id: sid,
            total_packets: 1,
            packet_size: 1024,
            total_size: d.len() as u32,
            data_hash: *blake3::hash(d).as_bytes(),
        };
        mgr.handle_spec(peer, spec(b'a', &data_a));
        mgr.handle_spec(peer, spec(b'b', &data_b));

        // Deliver both final packets (order intentionally b-then-a to prove drain isn't positional).
        mgr.handle_data(
            peer,
            PTData { stream_id: b'b', sequence: 0, payload: data_b.clone() },
        );
        mgr.handle_data(
            peer,
            PTData { stream_id: b'a', sequence: 0, payload: data_a.clone() },
        );

        // Drain by stream — each must yield ITS OWN payload, not whichever is first in the vec.
        assert!(mgr.check_inbound_complete(peer, b'a').is_some());
        assert!(mgr.check_inbound_complete(peer, b'b').is_some());
        assert_eq!(mgr.take_inbound_data(peer, b'a'), Some(data_a));
        assert_eq!(mgr.take_inbound_data(peer, b'b'), Some(data_b));
    }

    // Helper to parse VSF section fields (for legacy format like pt_spec)
    fn parse_vsf_section_fields(bytes: &[u8]) -> Vec<(String, vsf::VsfType)> {
        use vsf::file_format::VsfHeader;

        let (_, header_end) = match VsfHeader::decode(bytes) {
            Ok(h) => h,
            Err(_) => return vec![],
        };

        let mut ptr = header_end;
        let section = match vsf::VsfSection::parse(bytes, &mut ptr) {
            Ok(s) => s,
            Err(_) => return vec![],
        };

        section
            .fields
            .iter()
            .filter_map(|f| f.values.first().map(|v| (f.name.clone(), v.clone())))
            .collect()
    }

    // Helper to parse header-only PT packet fields Returns (provenance_hash, values) for the pt_* header field
    fn parse_pt_header_field(bytes: &[u8]) -> Option<([u8; 32], Vec<vsf::VsfType>)> {
        use vsf::file_format::VsfHeader;

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

        // Find pt_* header field and extract its inline values
        for field in &header.fields {
            if field.name.starts_with("pt_") && field.offset_bytes == 0 && field.size_bytes == 0 {
                return Some((provenance_hash, field.inline_values.clone()));
            }
        }

        None
    }
}
