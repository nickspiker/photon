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

/// Canonical form of a socket address for matching, collapsing an IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) to its plain IPv4 form. The OS hands the same peer back in different representations on different code paths — a transfer started to `a.b.c.d:port` gets its SPEC-ACK back from `[::ffff:a.b.c.d]:port`, and a raw `SocketAddr ==` treats those as different peers, so the ACK lands as "unknown stream" and the transfer never starts. Compare canonical forms everywhere a packet is routed to its transfer so the LAN/WAN race + lock-on actually works regardless of which representation the OS reports.
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
    /// Strict ordering per peer; head-of-line blocking is intentional (a stuck packet means the peer isn't answering, so nothing else would get thru either).
    outbound_packets: Vec<OutboundPacket>,
    /// Our keypair for signing
    keypair: Keypair,
    /// Stale timeout (no activity for this long = abort)
    stale_timeout: Duration,
    /// Next stream id per peer, rotating a..z by modulo. Per PEER, because the collision that matters is (peer, stream): a global counter let one peer's ids come back around while its earlier holder was still in flight.
    next_stream_id: crate::linear_map::LinearMap<PeerKey, u8>,
    /// Monotonic transfer ID counter for external tracking
    next_transfer_id: usize,
    /// Large payloads waiting for a slot in their peer's window, FIFO. Released in `tick()` on the completion/failure edge of an in-flight transfer.
    pending_outbound: std::collections::VecDeque<PendingOutbound>,
    /// Peers whose window is currently full — so the park and the drain each log once, not per transfer.
    window_parked: std::collections::HashSet<PeerKey>,
    /// Which device last spoke a signed SPEC from each address, oldest first, capped at `SPEC_SENDERS_CAP`.
    /// DATA carries no identity, so this is how a DATA arriving from an address its transfer was not keyed on is told apart: the same device path-migrating, or another device of the same friend reusing the letter (2026-10-21 Emma: four devices of one friend, 2958 hash mismatches).
    spec_senders: Vec<(SocketAddr, [u8; 32])>,
    /// Addresses whose reliable-packet head exhausted every retry without one ACK: dead until a datagram arrives FROM them (`heard_from`, the revival edge).
    /// Small sends to a dead address go to a live alternate or are refused, so one expired address can't hold a growing queue (2026-10-20 Nick: an expired temporary IPv6 took 2741 retransmits and 548 give-ups over three and a half hours).
    dead_addrs: Vec<SocketAddr>,
    /// Addresses that died since the network loop last asked ([`Self::take_newly_dead`]) — each one is an unreachable-peer edge for the app.
    newly_dead: Vec<SocketAddr>,
    /// Inbound transfers already answered with a COMPLETE, oldest first, capped at `SPEC_SENDERS_CAP`: a COMPLETE is one unacknowledged datagram, so the sender's probe (its final chunk again) is answered from here when it was lost.
    /// An entry leaves when its sender opens a new transfer on the same letter (the sender has retired the old one by then), or off the cap.
    answered: Vec<Answered>,
}

/// One inbound transfer this receiver has answered: whose it was and what the answer said.
struct Answered {
    stream_id: u8,
    sender: Option<[u8; 32]>,
    paths: Vec<SocketAddr>,
    complete: PTComplete,
    /// Whether a probe has been answered from this entry yet, so the re-answer logs once.
    reanswered: bool,
}

/// How a peer is told apart for the send window and its stream-id rotation: by device key when the caller gave one (a retarget moves the address, the window must not care), else by address.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum PeerKey {
    Device([u8; 32]),
    Addr(SocketAddr),
}

impl std::fmt::Display for PeerKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PeerKey::Device(pk) => write!(f, "device {}", hex::encode(&pk[..4])),
            PeerKey::Addr(a) => write!(f, "{}", a),
        }
    }
}

fn peer_key(recipient_pubkey: Option<[u8; 32]>, peer_addr: SocketAddr) -> PeerKey {
    match recipient_pubkey {
        Some(pk) => PeerKey::Device(pk),
        None => PeerKey::Addr(crate::network::udp::canon_socketaddr(peer_addr)),
    }
}

/// A large payload parked behind the window: everything `start_transfer` needs, held verbatim.
struct PendingOutbound {
    peer_addr: SocketAddr,
    alt_addr: Option<SocketAddr>,
    data: Vec<u8>,
    recipient_pubkey: Option<[u8; 32]>,
    tag: Option<[u8; 32]>,
}

/// One transfer's progress for the UI: which peer, how far, which way, and — when the sender named it — what it carries.
#[derive(Clone, Debug, PartialEq)]
pub struct TransferProgress {
    pub peer: SocketAddr,
    pub done: u32,
    pub total: u32,
    pub outbound: bool,
    /// The sender's tag (an attachment's content hash); None for untagged sends and for every inbound transfer, whose payload is unknown until it lands.
    pub tag: Option<[u8; 32]>,
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
            next_stream_id: crate::linear_map::LinearMap::new(),
            next_transfer_id: 0,
            pending_outbound: std::collections::VecDeque::new(),
            window_parked: std::collections::HashSet::new(),
            spec_senders: Vec::new(),
            dead_addrs: Vec::new(),
            answered: Vec::new(),
            newly_dead: Vec::new(),
        }
    }

    /// Get reference to keypair (for relay fallback)
    pub fn keypair(&self) -> &Keypair {
        &self.keypair
    }

    // ========================================================================= Transfer Stream Management ('a'-'z') =========================================================================

    /// THE SEND WINDOW (Nick 2026-09-21, the 345 MB pigeon that never showed up): at most this many large transfers in flight per peer — half the stream alphabet, a..m — so the other half is cooling.
    /// The stream id space is 26 per peer and the RECEIVER evicts an incomplete inbound transfer when a new SPEC arrives on its stream id; 1383 chunks fired at once wrapped the alphabet fifty times over and the chunks killed each other mid-flight (62 of 1383 landed, 1609 "Transfer FAILED").
    /// With ids rotating by modulo and at most thirteen live, an id comes back around only after twenty-six allocations, and its previous holder was retired at least thirteen releases ago — a late packet from it finds no live twin.
    pub const STREAMS_IN_FLIGHT: usize = 13;

    /// Allocate this peer's next stream id, a..z rotating by modulo.
    fn allocate_stream_id(&mut self, key: PeerKey) -> u8 {
        let slot = self.next_stream_id.entry(key).or_insert(0);
        let id = b'a' + *slot;
        *slot = (*slot + 1) % 26;
        id
    }

    /// Large transfers live for this peer right now (a completed or failed one has left the window even before the sweep removes it).
    fn in_flight_for(&self, key: PeerKey) -> usize {
        self.outbound
            .iter()
            .filter(|t| peer_key(t.recipient_pubkey, t.peer_addr) == key)
            .filter(|t| !matches!(t.state, TransferState::Complete | TransferState::Failed))
            .count()
    }

    /// Max VSF size for single UDP packet (no sharding needed) 1KB threshold - VSF this size or smaller sent directly Larger VSF gets sharded into [lowercase letter][packet number][1KB DATA] packets
    pub const SINGLE_PACKET_MAX: usize = 1024;
    /// Retry cap for a reliable small packet before the stop-and-wait head is dropped and the per-peer FIFO advances. With 1→2→…→60s backoff, ~5 retries ≈ 30-60s of trying — long enough to ride out a brief blip, short enough that an undeliverable head (dead avatar request) can't blackhole the chat queued behind it. The higher layer re-queues (chat retransmit / avatar→FGTW), so a drop is a deferral, not a loss.
    pub const MAX_PACKET_RETRIES: u32 = 5;
    /// How many addresses `spec_senders` and `dead_addrs` each remember (and how many transfers `answered` does): tens of friend devices times their few addresses, with headroom for rotating temporary IPv6 addresses.
    /// The oldest falls off first.
    const SPEC_SENDERS_CAP: usize = 256;

    /// Remember that `sender` signed a SPEC from `addr`.
    /// A later SPEC from the same address replaces the entry, so a reassigned LAN address follows its new device.
    fn note_spec_sender(&mut self, addr: SocketAddr, sender: Option<[u8; 32]>) {
        let Some(dev) = sender else { return };
        self.spec_senders.retain(|(a, _)| !same_addr(*a, addr));
        self.spec_senders.push((canon_addr(addr), dev));
        if self.spec_senders.len() > Self::SPEC_SENDERS_CAP {
            self.spec_senders.remove(0);
        }
    }

    /// The device that last signed a SPEC from `addr`, if any did.
    fn spec_sender_at(&self, addr: SocketAddr) -> Option<[u8; 32]> {
        self.spec_senders.iter().find(|(a, _)| same_addr(*a, addr)).map(|(_, d)| *d)
    }

    /// True when a DATA ACK's chunk hash names the bytes `t` sent at that sequence (the receiver hashes exactly the payload it got).
    fn ack_names_chunk(t: &OutboundTransfer, ack: &PTAck) -> bool {
        t.send_buffer.get_packet(ack.sequence).map_or(false, |p| *blake3::hash(p).as_bytes() == ack.chunk_hash)
    }

    /// True when `addr` stopped answering and has not been heard from since.
    fn is_dead(&self, addr: SocketAddr) -> bool {
        self.dead_addrs.iter().any(|d| same_addr(*d, addr))
    }

    /// The revival edge: any datagram FROM an address proves it answers again, so sends to it resume.
    /// The receive loop calls this for every datagram; it is a no-op while nothing is dead.
    /// The addresses that died since the last call, for the network loop to report as unreachable-peer edges.
    pub fn take_newly_dead(&mut self) -> Vec<SocketAddr> {
        std::mem::take(&mut self.newly_dead)
    }

    pub fn heard_from(&mut self, addr: SocketAddr) {
        if let Some(i) = self.dead_addrs.iter().position(|d| same_addr(*d, addr)) {
            self.dead_addrs.swap_remove(i);
            crate::logf!("PT: heard from {} again — sends to it resume", addr);
        }
    }

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
        self.send_tagged(peer_addr, alt_addr, data, recipient_pubkey, None)
    }

    /// [`send_with_pubkey_and_alt`](Self::send_with_pubkey_and_alt) with a caller's `tag` naming what the payload carries — reported back by [`transfer_progress`](Self::transfer_progress) so the UI can attribute a sharded transfer to one blob.
    pub fn send_tagged(
        &mut self,
        peer_addr: SocketAddr,
        alt_addr: Option<SocketAddr>,
        data: Vec<u8>,
        recipient_pubkey: Option<[u8; 32]>,
        tag: Option<[u8; 32]>,
    ) -> Vec<u8> {
        // NEVER accept the unspecified sentinel (`0.0.0.0:0` / `[::]:0` — `status::RELAY_ADDR`). Callers hand it deliberately for a relay-only peer, meaning "there is no direct address; the relay_to fan-out delivers this".
        // A fire-and-forget path (presence ping) can shrug that off — one datagram to nowhere costs nothing. A RELIABLE queue cannot: the packet can never be ACKed (nothing listens at the zero address), so it burns the full 1→3→5→10→29s retry ladder and, because the queue is stop-and-wait PER PEER, it HEAD-OF-LINE-BLOCKS every real message queued behind it — the "messages stick" failure the give-up sweep in `tick()` was added to paper over.
        // Measured before this guard (submitted logs, 2026-07-21..27): 42,820 zero-address retransmits on one device, 2,830 on another — 86% of ALL its PT retransmits, every one undeliverable by construction.
        // The history drain in `status.rs` already refused the sentinel for exactly this reason ("relay-only pairs starved forever on it"); chat never got the same guard. This is that guard, moved DOWN to the layer that owns the queue so no future caller can reintroduce it.
        // Returning empty is the correct contract: it means "nothing to send on the wire right now", which every caller already handles (it's the same value a packet queued behind an in-flight head returns). The relay fan-out that follows the call site is untouched, so the message still goes out — it just no longer leaves a poisoned entry in the reliable queue.
        if crate::network::traverse::gather::is_bogus_addr(&peer_addr) {
            return Vec::new();
        }

        // Small payload — enqueue as a reliable packet (stop-and-wait, one in flight per peer, retransmitted on backoff in tick() until the receiver's delivery ack arrives). Returns the bytes to send NOW only if no packet is already in flight to this peer; otherwise it queues behind the in-flight head and goes out when that head is acked.
        if data.len() <= Self::SINGLE_PACKET_MAX {
            // A dead primary hands the send to a live alternate, else the send is refused: nothing queues behind an address that stopped answering, and the first datagram heard from it lifts the refusal.
            // Returning empty is the same contract as the sentinel guard above; the caller's relay fan-out and retransmit sweep are untouched.
            let (peer_addr, alt_addr) = match (self.is_dead(peer_addr), alt_addr.filter(|a| !self.is_dead(*a))) {
                (false, alt) => (peer_addr, alt),
                (true, Some(alt)) => (alt, None),
                (true, None) => return Vec::new(),
            };
            let peer_busy = self
                .outbound_packets
                .iter()
                .any(|p| same_addr(p.peer_addr, peer_addr) && p.in_flight);
            let mut pkt = OutboundPacket::new(peer_addr, alt_addr, data, recipient_pubkey);
            // The same bytes already queued for this address are the same delivery: a higher layer's retransmit must not stack a copy behind its own original.
            if self.outbound_packets.iter().any(|p| p.packet_hash == pkt.packet_hash && same_addr(p.peer_addr, peer_addr)) {
                return Vec::new();
            }
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

        // Large payload - full SPEC/DATA/ACK/COMPLETE flow, behind the per-peer window.
        let key = peer_key(recipient_pubkey, peer_addr);
        if self.in_flight_for(key) >= Self::STREAMS_IN_FLIGHT {
            if self.window_parked.insert(key) {
                crate::logf!("PT: window full toward {} ({} in flight) — queuing behind it", peer_addr, Self::STREAMS_IN_FLIGHT);
            }
            self.pending_outbound.push_back(PendingOutbound { peer_addr, alt_addr, data, recipient_pubkey, tag });
            return Vec::new();
        }
        self.start_transfer(peer_addr, alt_addr, data, recipient_pubkey, tag)
    }

    /// Open a large transfer now (the window has room): allocate the peer's next stream id, build and mark the SPEC, track it. Returns the SPEC bytes.
    fn start_transfer(
        &mut self,
        peer_addr: SocketAddr,
        alt_addr: Option<SocketAddr>,
        data: Vec<u8>,
        recipient_pubkey: Option<[u8; 32]>,
        tag: Option<[u8; 32]>,
    ) -> Vec<u8> {
        let stream_id = self.allocate_stream_id(peer_key(recipient_pubkey, peer_addr));
        let transfer_id = self.next_transfer_id;
        self.next_transfer_id += 1;

        let mut transfer = OutboundTransfer::new(peer_addr, data, stream_id, transfer_id);
        // Don't race against the same address twice (caller may pass equal LAN/WAN).
        transfer.alt_addr = alt_addr.filter(|a| *a != peer_addr);
        transfer.tag = tag;

        // Store pubkey for relay fallback
        if let Some(pubkey) = recipient_pubkey {
            transfer.set_recipient_pubkey(pubkey);
        }

        let spec = transfer.build_spec();
        let spec_bytes = spec.to_vsf_bytes(&self.keypair);

        // Mark SPEC as sent for retry tracking
        transfer.mark_spec_sent();

        crate::logf!(
            "PT: Starting outbound transfer #{} to {} ({} bytes, stream '{}', relay={})",
            transfer_id,
            peer_addr,
            transfer.send_buffer.total_size(),
            stream_id as char,
            recipient_pubkey.is_some()
        );

        // Push to vec - allows multiple concurrent transfers to same peer
        self.outbound.push(transfer);

        spec_bytes
    }

    /// Path truth changed for a device (a pong/TRAVERSE validation just proved fresh addresses): re-aim every queued/in-flight small packet and every still-unlocked transfer for that pubkey at the proven pair. THE FROZEN-ADDRESS FIX (field 2026-09-02): addresses were captured at enqueue and never revisited, so retransmit ladders burned entire backoff runs against a stale wrong-subnet v4, a dead cellular v6, and a firewall-blocked path while the validated working path sat idle — a same-LAN self-message took 60s-to-minutes and delivered only when a later re-serve happened to enqueue at the fresh address. Transfers that already locked (spec_acked — the peer ANSWERED on that path) are left alone; packet-hash acks are address-blind so a retargeted in-flight head still completes cleanly.
    pub fn retarget_peer(
        &mut self,
        pubkey: &[u8; 32],
        primary: SocketAddr,
        alt: Option<SocketAddr>,
    ) {
        if crate::network::traverse::gather::is_bogus_addr(&primary) {
            return;
        }
        let alt = alt.filter(|a| !same_addr(*a, primary));
        let mut moved = 0usize;
        for pkt in self.outbound_packets.iter_mut() {
            if pkt.recipient_pubkey.as_ref() == Some(pubkey) && !same_addr(pkt.peer_addr, primary) {
                pkt.peer_addr = primary;
                pkt.alt_addr = alt;
                moved += 1;
            }
        }
        for t in self.outbound.iter_mut() {
            if !t.spec_acked
                && t.recipient_pubkey.as_ref() == Some(pubkey)
                && !same_addr(t.peer_addr, primary)
            {
                t.peer_addr = primary;
                t.alt_addr = alt;
                moved += 1;
            }
        }
        for p in self.pending_outbound.iter_mut() {
            if p.recipient_pubkey.as_ref() == Some(pubkey) && !same_addr(p.peer_addr, primary) {
                p.peer_addr = primary;
                p.alt_addr = alt;
                moved += 1;
            }
        }
        if moved > 0 {
            let alt_s = alt.map_or("none".to_string(), |a| a.to_string());
            crate::logf!(
                "PT: retargeted {} queued item(s) for {} onto validated path {} (alt {})",
                moved,
                hex::encode(&pubkey[..4]),
                primary,
                alt_s
            );
        }
    }

    /// Handle received SPEC (start receiving)
    /// A SPEC we already hold the transfer for — the same stream and hash from ANY address (a six-path anchor delivers one SPEC six times; a lost SPEC ACK makes the sender repeat it). The ACK still goes back so the sender locks onto whichever path answered, but no second transfer and no second log line (2026-10-02, the Emma wave: 222 transfer lines in one second in front of the media).
    /// The answering address joins that transfer's declared paths: the hash names the exact bytes, so DATA from any address that declared this SPEC belongs to it, whichever device sent it.
    pub fn spec_known(&mut self, peer_addr: SocketAddr, spec: &PTSpec, sender: Option<[u8; 32]>) -> bool {
        self.note_spec_sender(peer_addr, sender);
        let Some(t) = self.inbound.iter_mut().find(|t| {
            t.stream_id == spec.stream_id && t.receive_buffer.expected_hash() == spec.data_hash && t.state != TransferState::Failed
        }) else {
            return false;
        };
        if !t.paths.iter().any(|p| same_addr(*p, peer_addr)) {
            t.paths.push(peer_addr);
        }
        true
    }

    /// The SPEC ACK alone, for a SPEC already known (see `spec_known`).
    pub fn spec_ack_bytes(&self, spec: &PTSpec) -> Vec<u8> {
        PTAck {
            stream_id: spec.stream_id,
            sequence: u32::MAX,
            chunk_hash: spec.data_hash,
        }
        .to_vsf_bytes(&self.keypair)
    }

    /// `sender` is the device key that signed the SPEC (None only in tests and for unsigned callers): it owns the transfer, not the address it arrived on.
    pub fn handle_spec(&mut self, peer_addr: SocketAddr, spec: PTSpec, sender: Option<[u8; 32]>) -> Vec<u8> {
        crate::logf!(
            "PT: Received SPEC from {} - stream '{}', {} packets, {} bytes, hash {}",
            peer_addr,
            spec.stream_id as char,
            spec.total_packets,
            spec.total_size,
            hex::encode(&spec.data_hash[..4])
        );

        let stream_id = spec.stream_id;
        self.note_spec_sender(peer_addr, sender);

        // Remove any existing incomplete transfer this SENDER held on this stream: a new SPEC means it abandoned the old one.
        // By device when the SPEC is signed (its old transfer may be keyed at another of its addresses), by address otherwise; never another device's transfer that merely shares the letter, since every sender rotates its own a..z.
        self.inbound.retain(|t| {
            let same_sender = match (sender, t.sender) {
                (Some(a), Some(b)) => a == b,
                _ => same_addr(t.peer_addr, peer_addr),
            };
            !(same_sender && t.stream_id == stream_id && !t.is_complete())
        });
        // The same sender reusing the letter for other bytes has retired that letter's earlier transfer (its window keeps a letter's holders thirteen releases apart), so the earlier answer is no longer anyone's to probe for.
        // Same bytes is a late copy of the answered SPEC, not a new transfer: that answer stays.
        self.answered.retain(|a| {
            let same_sender = match (sender, a.sender) {
                (Some(x), Some(y)) => x == y,
                _ => a.paths.iter().any(|p| same_addr(*p, peer_addr)),
            };
            !(same_sender && a.stream_id == stream_id && a.complete.final_hash != spec.data_hash)
        });

        let mut transfer = InboundTransfer::new(peer_addr, &spec);
        transfer.sender = sender;
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

        // Find the transfer by stream_id, accepting the ACK from the primary path, the raced alternate, OR — when the stream id is unambiguous — any third address (a multi-homed receiver's egress can differ from the ingress we aimed at; 2026-09-02 field). Whichever address answered is the reachable one, so lock the transfer onto it and drop the alternate — DATA/ACK route by (peer_addr, stream_id), so all subsequent packets must use the path that ACKed.
        // The ACK echoes the SPEC's data hash, so both matches require it: a SPEC ACK for another transfer on the same letter (another recipient device, an earlier lap of the alphabet) can never start or re-aim this one.
        let matched = self
            .outbound
            .iter()
            .position(|t| {
                t.stream_id == stream_id
                    && t.send_buffer.data_hash() == data_hash
                    && (same_addr(t.peer_addr, peer_addr)
                        || t.alt_addr.map_or(false, |a| same_addr(a, peer_addr)))
            })
            .or_else(|| {
                let mut candidates = self
                    .outbound
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.stream_id == stream_id && !t.spec_acked && t.send_buffer.data_hash() == data_hash);
                match (candidates.next(), candidates.next()) {
                    (Some((i, _)), None) => Some(i),
                    _ => None,
                }
            });
        if let Some(transfer) = matched.map(|i| &mut self.outbound[i]) {
            // A repeat SPEC ACK (the SPEC ladder fired again before the first ACK was back) changes nothing: it must not pull a transfer that is already waiting on its COMPLETE, or already has it, back to Transferring.
            if transfer.spec_acked {
                return packets;
            }
            if !same_addr(transfer.peer_addr, peer_addr) {
                crate::logf!("PT: SPEC ACK arrived on alternate path {} (was {}) for stream '{}' - locking onto it", peer_addr, transfer.peer_addr, stream_id as char);
                transfer.peer_addr = peer_addr;
            }
            transfer.alt_addr = None;
            transfer.spec_acked = true;
            transfer.state = TransferState::Transferring;
            transfer.last_activity = Instant::now();
            // Fresh stale budget for the just-proven path: whatever was burned before the lock (SPEC rounds against a dead primary can run 10+ seconds) must not bill the DATA phase.
            transfer.retries = 0;

            crate::logf!(
                "PT: SPEC ACK received from {} for stream '{}', starting DATA transfer",
                peer_addr,
                stream_id as char
            );

            // Send initial window of DATA packets
            for data in transfer.packets_to_send() {
                packets.push(data.to_bytes());
            }
        } else {
            crate::logf!(
                "PT: SPEC ACK from {} for unknown stream '{}' (hash {})",
                peer_addr,
                stream_id as char,
                hex::encode(&data_hash[..4])
            );
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
        // Find the inbound transfer by stream_id and a path that declared it (its SPEC's source, or any address that sent the same SPEC hash).
        // Falling back to a UNIQUE stream match with address adoption: a multi-homed sender's SPEC can arrive via one path (the v6 GUA) and its DATA via another (the v4 LAN), and keyed strictly by source address the DATA matched nothing and dropped as "unknown bytes" FOREVER while the sender retransmitted — the 2026-09-02 field wedge.
        // Adoption is by IDENTITY, never by letter alone: stream letters are each sender's own a..z, so several devices of one friend hold the same letter at once, and a letter-only adoption spliced one device's straggler DATA into another device's fresh transfer (2026-10-21 Emma: 79 adoptions bouncing transfers between .156 and .161, 2958 "Hash mismatch" lines, 66 inbound timeouts, 432 attachment re-asks).
        // An address that has signed a SPEC belongs to that device and adopts only into that device's transfer; an address that never has adopts only into a transfer that has received nothing yet (the 2026-09-02 shape, where the SPEC path and the DATA path split on the very first packet).
        let strict = self.inbound.iter().position(|t| {
            t.stream_id == data.stream_id && !t.is_complete() && t.paths.iter().any(|p| same_addr(*p, peer_addr))
        });
        let idx = strict.or_else(|| {
            let speaker = self.spec_sender_at(peer_addr);
            let mut candidates = self.inbound.iter().enumerate().filter(|(_, t)| {
                t.stream_id == data.stream_id
                    && !t.is_complete()
                    && match speaker {
                        Some(dev) => t.sender == Some(dev),
                        None => t.progress().0 == 0,
                    }
            });
            match (candidates.next(), candidates.next()) {
                (Some((i, _)), None) => Some(i),
                _ => None,
            }
        });
        if let Some(transfer) = idx.map(|i| &mut self.inbound[i]) {
            if strict.is_none() {
                crate::logf!(
                    "PT: DATA for stream '{}' arrived from {} (transfer was keyed {}) — sender path-migrated, adopting the live address",
                    data.stream_id as char,
                    peer_addr,
                    transfer.peer_addr
                );
                transfer.paths.push(peer_addr);
            }
            // The live address is where completion is looked up and answered (`check_inbound_complete`, `take_inbound_data`).
            transfer.peer_addr = peer_addr;
            if let Some(ack) = transfer.handle_data(&data) {
                let (recv, total) = transfer.progress();
                // Log at milestones: every 50 packets (but not 0) or completion
                if recv == total || (recv > 0 && recv % 50 == 0) {
                    crate::logf!(
                        "PT: Received {}/{} from {} stream '{}'",
                        recv,
                        total,
                        peer_addr,
                        data.stream_id as char
                    );
                }

                return Some(ack.to_vsf_bytes(&self.keypair));
            }
            return None;
        }
        // No live transfer claims it: a chunk of a transfer already answered is the sender's probe for a COMPLETE it never got (or a retransmit that crossed its last ACK), so the answer is that COMPLETE again.
        // Newest first, by the transfer's paths or by the device that signed SPECs from this address; a stale match names a hash the sender no longer holds and is ignored there.
        let speaker = self.spec_sender_at(peer_addr);
        let a = self.answered.iter_mut().rev().find(|a| {
            a.stream_id == data.stream_id
                && (a.paths.iter().any(|p| same_addr(*p, peer_addr)) || (speaker.is_some() && a.sender == speaker))
        })?;
        if !a.reanswered {
            a.reanswered = true;
            crate::logf!(
                "PT: a chunk of answered stream '{}' from {} — its COMPLETE was lost, answering again (hash {})",
                data.stream_id as char,
                peer_addr,
                hex::encode(&a.complete.final_hash[..4])
            );
        }
        Some(a.complete.to_vsf_bytes(&self.keypair))
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

        // Find outbound transfer by peer AND stream_id — with the same unique-stream fallback as handle_data: an ACK returning from an address we never sent to (the receiver's egress differs from its ingress — the multi-homed v4/v6 asymmetry) must lock the transfer onto the path that is demonstrably ALIVE, not be dropped.
        // Both matches require the ACK's chunk hash to name the bytes this transfer sent at that sequence, so an ACK for another transfer on the same letter (another recipient device, an earlier lap of the alphabet) never marks this one's chunk delivered.
        let strict = self.outbound.iter().position(|t| {
            same_addr(t.peer_addr, peer_addr)
                && t.stream_id == ack.stream_id
                && t.state == TransferState::Transferring
                && Self::ack_names_chunk(t, &ack)
        });
        let idx = strict.or_else(|| {
            let mut candidates = self.outbound.iter().enumerate().filter(|(_, t)| {
                t.stream_id == ack.stream_id && t.state == TransferState::Transferring && Self::ack_names_chunk(t, &ack)
            });
            match (candidates.next(), candidates.next()) {
                (Some((i, _)), None) => Some(i),
                _ => None,
            }
        });
        if let Some(transfer) = idx.map(|i| &mut self.outbound[i]) {
            if !same_addr(transfer.peer_addr, peer_addr) {
                crate::logf!(
                    "PT: ACK for stream '{}' arrived from {} (transfer aimed at {}) — locking onto the answering path",
                    ack.stream_id as char,
                    peer_addr,
                    transfer.peer_addr
                );
                transfer.peer_addr = peer_addr;
                transfer.alt_addr = None;
            }
            transfer.handle_ack(&ack);

            // Only log progress at milestones (every 100 packets or completion) Avoids spamming logs with per-ACK updates
            let (acked, total) = transfer.send_buffer.progress();
            if acked == total {
                crate::logf!(
                    "PT: All {}/{} ACK'd to {} stream '{}'",
                    acked,
                    total,
                    peer_addr,
                    ack.stream_id as char
                );
            } else if acked > 0 && acked % 100 == 0 {
                crate::logf!(
                    "PT: Progress {}/{} to {} stream '{}'",
                    acked,
                    total,
                    peer_addr,
                    ack.stream_id as char
                );
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
            crate::logf!(
                "PT: NAK received from {} - retransmitting {} packets",
                peer_addr,
                nak.missing_sequences.len()
            );

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
                crate::logf!("PT: Peer {} aborted transfer", peer_addr);
                self.outbound.retain(|t| !same_addr(t.peer_addr, peer_addr));
                self.inbound.retain(|t| !same_addr(t.peer_addr, peer_addr));
            }
            ControlCommand::Pause => {
                // Could pause sending, but for now just log
                crate::logf!("PT: Peer {} requested pause", peer_addr);
            }
            ControlCommand::Resume => {
                crate::logf!("PT: Peer {} requested resume", peer_addr);
            }
            ControlCommand::SlowDown => {
                if let Some(transfer) = self.outbound.iter_mut().find(|t| {
                    same_addr(t.peer_addr, peer_addr) && t.state == TransferState::Transferring
                }) {
                    transfer.window.on_loss(); // Treat SlowDown like loss - backs off send ratio
                    crate::logf!("PT: Slowing down to {}", peer_addr);
                }
            }
        }
    }

    /// Handle received COMPLETE
    pub fn handle_complete(&mut self, peer_addr: SocketAddr, complete: PTComplete) {
        // Find transfer by peer and final_hash; one already settled ignores a repeat (a probe and a straggler can each draw the same COMPLETE).
        if let Some(transfer) = self.outbound.iter_mut().find(|t| {
            same_addr(t.peer_addr, peer_addr)
                && t.send_buffer.data_hash() == complete.final_hash
                && !matches!(t.state, TransferState::Complete | TransferState::Failed)
        }) {
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

                crate::logf!("PT: → {} OK | {} | {:.1}s | {}B pkt | win {} | RTT {}ms | {:.0}% util ({} retx)", peer_addr, thruput_str, duration_ms as f64 / 1000.0, packet_size, max_window, rtt_ms, utilization, retransmits);
            } else {
                crate::logf!(
                    "PT: → {} FAILED verification ({} packets, {} bytes)",
                    peer_addr,
                    packets,
                    bytes
                );
            }
        }
    }

    /// Check if a SPECIFIC inbound transfer (peer + stream) is complete, return its COMPLETE packet.
    /// Stream-scoped: a peer can have several concurrent transfers (e.g. a CLUTCH offer AND a KEM response in flight at once), and they must not be confused — matching by address alone grabs whichever happens to be first in the vec, which silently drops the other.
    pub fn check_inbound_complete(
        &mut self,
        peer_addr: SocketAddr,
        stream_id: u8,
    ) -> Option<Vec<u8>> {
        if let Some(transfer) = self.inbound.iter_mut().find(|t| {
            same_addr(t.peer_addr, peer_addr)
                && t.stream_id == stream_id
                && t.is_complete()
                && t.state != TransferState::Failed
        }) {
            let complete = transfer.build_complete();
            // A failed assembly is final: retire it on this edge so the next tick sweeps it, instead of re-verifying, re-logging and re-answering it for every straggler DATA until the 30 s stale sweep (2026-10-21 Emma: 2958 "Hash mismatch" lines from about a dozen bad assemblies).
            if !complete.success {
                transfer.state = TransferState::Failed;
            }
            let bytes = complete.to_vsf_bytes(&self.keypair);
            let answered = Answered { stream_id, sender: transfer.sender, paths: transfer.paths.clone(), complete, reanswered: false };
            self.note_answered(answered);
            return Some(bytes);
        }
        None
    }

    /// Remember an answered transfer for the sender's probe (see `answered`), replacing any earlier answer for the same letter and bytes.
    fn note_answered(&mut self, a: Answered) {
        self.answered.retain(|o| !(o.stream_id == a.stream_id && o.complete.final_hash == a.complete.final_hash && o.sender == a.sender));
        self.answered.push(a);
        if self.answered.len() > Self::SPEC_SENDERS_CAP {
            self.answered.remove(0);
        }
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
            crate::logf!(
                "PT: Cleared {} outbound transfers to {} (forced)",
                removed,
                peer_addr
            );
        }
        // The ladder is parked for this peer: what waited behind the window goes with it (the sender's re-drop or re-ask is the retry, as for the transfers themselves).
        self.pending_outbound.retain(|p| !same_addr(p.peer_addr, *peer_addr));
    }

    /// Periodic tick - check timeouts, send retransmits Returns TickSend structs with:
    /// - peer_addr, wire_bytes: UDP packet to send (the preferred path)
    /// - tcp_payload: if Some, also send this whole VSF over TCP (reliable fallback, once per transfer)
    /// - relay: if Some, UDP+TCP failed, relay via /conduit with this info
    /// Live progress of every ACTIVE sharded transfer: (peer, done, total, outbound). Small single-packet sends never appear (they have no SPEC). Drives the attachment progress bar; callers filter by size/peer.
    pub fn transfer_progress(&self) -> Vec<TransferProgress> {
        let mut out = Vec::new();
        for t in &self.outbound {
            let (done, total) = t.send_buffer.progress();
            if done < total {
                out.push(TransferProgress { peer: t.peer_addr, done, total, outbound: true, tag: t.tag });
            }
        }
        // A tagged transfer still waiting behind the window is part of its blob too — reported at zero, so a bar never counts it as already sent.
        for p in &self.pending_outbound {
            if p.tag.is_some() {
                out.push(TransferProgress { peer: p.peer_addr, done: 0, total: 1, outbound: true, tag: p.tag });
            }
        }
        for t in &self.inbound {
            let (done, total) = t.receive_buffer.progress();
            if done < total {
                out.push(TransferProgress { peer: t.peer_addr, done, total, outbound: false, tag: None });
            }
        }
        out
    }

    pub fn tick(&mut self) -> Vec<TickSend> {
        let mut to_send = Vec::new();

        // Check outbound transfers
        for transfer in &mut self.outbound {
            // Complete and Failed are done; the sweep below removes them.
            if matches!(transfer.state, TransferState::Complete | TransferState::Failed) {
                continue;
            }
            // Every chunk ACK'd, the COMPLETE not yet heard: the probe rides the DATA retransmit ladder (see `OutboundTransfer::handle_ack`), and the receiver answers it with the COMPLETE again.
            // This state used to be skipped outright, so a lost COMPLETE held a window slot forever (2026-10-21).
            // When the ladder runs out unanswered the receiver has forgotten the transfer (restarted, or its memory moved on): every chunk was ACK'd, so the transfer retires quietly instead of logging a timeout, and the higher layer's re-ask is the retry, as for any failure.
            if transfer.state == TransferState::AwaitingComplete {
                if transfer.is_stale(self.stale_timeout) {
                    crate::logf!(
                        "PT: stream '{}' to {} — every chunk ACK'd but no COMPLETE answered {} probe(s); retiring it unconfirmed",
                        transfer.stream_id as char,
                        transfer.peer_addr,
                        transfer.retries
                    );
                    transfer.state = TransferState::Failed;
                    continue;
                }
                for data in transfer.check_timeouts() {
                    to_send.push(TickSend { peer_addr: transfer.peer_addr, wire_bytes: data.to_bytes(), tcp_payload: None, relay: None });
                }
                continue;
            }
            if transfer.is_stale(self.stale_timeout) {
                crate::logf!("PT: Outbound transfer to {} timed out", transfer.peer_addr);
                transfer.state = TransferState::Failed;
                continue;
            }

            // SPEC retry with exponential backoff
            if transfer.spec_needs_retry() {
                // After 1s, also try TCP in parallel — but send the WHOLE VSF over TCP exactly once (not the SPEC shard, and not every retry). TCP is the reliable fallback; UDP sharding stays preferred and keeps going in parallel until one path ACKs.
                let tcp_eligible = transfer.tcp_eligible();
                let tcp_payload = if tcp_eligible && !transfer.tcp_sent {
                    transfer.set_spec_tcp_fallback();
                    transfer.tcp_sent = true;
                    crate::logf!("PT: SPEC for stream '{}' to {} - sending whole payload over TCP (fallback, once)", transfer.stream_id as char, transfer.peer_addr);
                    transfer.original_payload.clone()
                } else {
                    None
                };

                // Check if we should try relay (UDP+TCP tried, no ACK) — ONCE per transfer: should_relay_fallback stays true every retry tick past the threshold, so guard on relay_sent to avoid re-uploading the whole payload each cycle.
                let use_relay = transfer.should_relay_fallback() && !transfer.relay_sent;
                if use_relay {
                    transfer.relay_sent = true;
                }

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
                            crate::logf!(
                                "PT: SPEC stream '{}' to {} - falling back to relay",
                                transfer.stream_id as char,
                                transfer.peer_addr
                            );
                            Some(RelayInfo {
                                recipient_pubkey: pubkey,
                                payload: payload.clone(),
                            })
                        }
                        _ => {
                            crate::logf!(
                                "PT: SPEC stream '{}' to {} - relay needed but no pubkey/payload",
                                transfer.stream_id as char,
                                transfer.peer_addr
                            );
                            None
                        }
                    }
                } else {
                    crate::logf!(
                        "PT: Retrying SPEC stream '{}' to {} (attempt {}, tcp={})",
                        transfer.stream_id as char,
                        transfer.peer_addr,
                        transfer.spec_retry_count,
                        transfer.spec_tcp_fallback
                    );
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
        let mut gone_dead: Vec<SocketAddr> = Vec::new();
        self.outbound_packets.retain(|pkt| {
            if pkt.in_flight && pkt.retry_count >= Self::MAX_PACKET_RETRIES {
                crate::logf!("PT: giving up on undeliverable packet to {} after {} retries — advancing the queue", pkt.peer_addr, pkt.retry_count);
                advanced_peers.push(pkt.peer_addr);
                for a in std::iter::once(pkt.peer_addr).chain(pkt.alt_addr) {
                    if !gone_dead.iter().any(|d| same_addr(*d, a)) {
                        gone_dead.push(a);
                    }
                }
                false
            } else {
                true
            }
        });
        // An exhausted head is the death edge for the addresses it raced: every retry went to them and not one ACK came back.
        // Mark them dead until heard from, re-aim what queued behind them at a live alternate where the sender named one, and drop the rest — the same deferral as the give-up itself, and with the address dead a higher layer's re-queue is refused instead of re-stacking a ~24 s ladder per packet (2026-10-20 Nick: 548 give-ups in a row against one expired temporary IPv6).
        let death_edge = !gone_dead.is_empty();
        for dead in gone_dead {
            if !self.is_dead(dead) {
                crate::logf!("PT: {} never answered — dead until heard from; small sends to it are refused", dead);
                self.dead_addrs.push(canon_addr(dead));
                self.newly_dead.push(dead);
                // An expired temporary address is never heard from again, so the list is capped: the oldest falls off and, if anyone still aims at it, dies again after one more ladder.
                if self.dead_addrs.len() > Self::SPEC_SENDERS_CAP {
                    self.dead_addrs.remove(0);
                }
            }
        }
        if death_edge {
            let dead_addrs = &self.dead_addrs;
            let is_dead = |a: SocketAddr| dead_addrs.iter().any(|d| same_addr(*d, a));
            let (mut dropped, mut reaimed) = (0usize, 0usize);
            self.outbound_packets.retain_mut(|p| {
                if p.alt_addr.map_or(false, |a| is_dead(a)) {
                    p.alt_addr = None;
                }
                if p.in_flight || !is_dead(p.peer_addr) {
                    return true;
                }
                match p.alt_addr.take() {
                    Some(alt) => {
                        p.peer_addr = alt;
                        reaimed += 1;
                        advanced_peers.push(alt);
                        true
                    }
                    None => {
                        dropped += 1;
                        false
                    }
                }
            });
            if dropped + reaimed > 0 {
                crate::logf!("PT: dropped {} queued packet(s) behind a dead address, re-aimed {} at a live alternate", dropped, reaimed);
            }
        }
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
                to_send.push(TickSend {
                    peer_addr: paddr,
                    wire_bytes: payload.clone(),
                    tcp_payload: None,
                    relay: None,
                });
                if let Some(alt) = alt {
                    to_send.push(TickSend {
                        peer_addr: alt,
                        wire_bytes: payload,
                        tcp_payload: None,
                        relay: None,
                    });
                }
            }
        }

        // Retransmit reliable small packets whose backoff has elapsed (stop-and-wait per peer: only in-flight heads retransmit; queued packets wait for their head to be acked). Raced LAN/WAN like streams. No TCP/relay here — a 60s-capped UDP retry is the reliability for small packets; if a peer is truly unreachable, nothing flows anyway.
        for pkt in self.outbound_packets.iter_mut() {
            if pkt.in_flight && pkt.needs_retransmit() {
                pkt.mark_retransmit();
                crate::logf!(
                    "PT: Retransmitting packet to {} (attempt {}, next backoff {}s)",
                    pkt.peer_addr,
                    pkt.retry_count,
                    pkt.next_delay.as_secs()
                );
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
                crate::logf!("PT: Inbound transfer from {} timed out", transfer.peer_addr);
                transfer.state = TransferState::Failed;
            }
        }

        // Remove failed transfers — and COMPLETED outbound ones, which nothing reads after the peer's pt_done (they used to linger with two copies of their payload until a CLUTCH completion or a relay-offline verdict cleared the peer: a 345 MB pigeon retained ~700 MB).
        self.outbound.retain(|t| !matches!(t.state, TransferState::Failed | TransferState::Complete));
        self.inbound.retain(|t| t.state != TransferState::Failed);

        // Release what waited behind a window that now has room: FIFO per peer, one SPEC per released transfer (to its alt address too, as the direct send does).
        let mut kept = std::collections::VecDeque::new();
        while let Some(p) = self.pending_outbound.pop_front() {
            let key = peer_key(p.recipient_pubkey, p.peer_addr);
            if self.in_flight_for(key) >= Self::STREAMS_IN_FLIGHT {
                kept.push_back(p);
                continue;
            }
            let spec_bytes = self.start_transfer(p.peer_addr, p.alt_addr, p.data, p.recipient_pubkey, p.tag);
            if let Some(alt) = p.alt_addr.filter(|a| !same_addr(*a, p.peer_addr)) {
                to_send.push(TickSend { peer_addr: alt, wire_bytes: spec_bytes.clone(), tcp_payload: None, relay: None });
            }
            to_send.push(TickSend { peer_addr: p.peer_addr, wire_bytes: spec_bytes, tcp_payload: None, relay: None });
        }
        self.pending_outbound = kept;
        let still_parked: std::collections::HashSet<PeerKey> = self.pending_outbound.iter().map(|p| peer_key(p.recipient_pubkey, p.peer_addr)).collect();
        for key in self.window_parked.clone() {
            if !still_parked.contains(&key) {
                crate::logf!("PT: window drained toward {} — nothing left queued", key);
                self.window_parked.remove(&key);
            }
        }

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

    /// The unspecified sentinel (`status::RELAY_ADDR`) must NEVER enter the reliable queue. Callers pass it on purpose for a relay-only peer, and a packet addressed there can never be ACKed — so it would burn the whole retry ladder and head-of-line-block every real message behind it in the per-peer FIFO. Submitted logs showed 42,820 such retransmits on a single device before the guard.
    /// Both size classes are checked: small payloads take the stop-and-wait packet path, large ones the SPEC/DATA stream path, and the guard sits ahead of both.
    #[test]
    fn bogus_addr_never_enqueues() {
        let mut mgr = PTManager::new(test_keypair());
        for addr in ["0.0.0.0:0", "[::]:0"] {
            let bogus: SocketAddr = addr.parse().unwrap();
            for len in [64usize, 8000] {
                assert!(
                    mgr.send(bogus, vec![0xCD; len]).is_empty(),
                    "{addr} ({len}B) must yield no wire bytes"
                );
            }
        }
        assert!(
            mgr.outbound_packets.is_empty(),
            "the sentinel must leave NOTHING queued — a queued packet is the head-of-line block itself"
        );

        // A real address still works — the guard must not be a blanket refusal.
        let real: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        assert!(
            !mgr.send(real, vec![0xCD; 64]).is_empty(),
            "a routable address must still send"
        );
    }

    /// THE FROZEN-ADDRESS FIX (2026-09-02): a proven fresh path must re-aim queued small packets and un-locked transfers keyed by recipient pubkey; a transfer that already locked (spec_acked) stays put.
    #[test]
    fn retarget_moves_queued_items_but_not_locked_transfers() {
        let mut mgr = PTManager::new(test_keypair());
        let stale: SocketAddr = "192.168.0.40:4383".parse().unwrap();
        let fresh: SocketAddr = "192.168.1.161:4383".parse().unwrap();
        let pk = [0x77u8; 32];
        // Small packet + large transfer, both aimed at the stale address.
        let _ = mgr.send_with_pubkey(stale, vec![1; 64], Some(pk));
        let _ = mgr.send_with_pubkey(stale, vec![2; 8000], Some(pk));
        // A second transfer for the SAME pubkey that already locked its path.
        let _ = mgr.send_with_pubkey(stale, vec![3; 8000], Some(pk));
        let locked_stream = mgr.outbound[1].stream_id;
        mgr.outbound[1].spec_acked = true;
        // A different peer's packet must not move.
        let other: SocketAddr = "10.0.0.9:4383".parse().unwrap();
        let _ = mgr.send_with_pubkey(other, vec![4; 64], Some([0x88u8; 32]));

        mgr.retarget_peer(&pk, fresh, None);

        assert!(same_addr(mgr.outbound_packets[0].peer_addr, fresh), "queued packet re-aimed");
        assert!(same_addr(mgr.outbound_packets[1].peer_addr, other), "other peer untouched");
        for t in &mgr.outbound {
            if t.stream_id == locked_stream {
                assert!(same_addr(t.peer_addr, stale), "locked transfer keeps its answering path");
            } else {
                assert!(same_addr(t.peer_addr, fresh), "un-locked transfer re-aimed");
            }
        }
    }

    /// The multi-homed wedge: SPEC arrives via one path, DATA via another. With exactly one incomplete inbound transfer on the stream, the DATA must match and the transfer must adopt the live source instead of dropping as "unknown bytes" forever.
    #[test]
    fn inbound_data_adopts_a_migrated_sender_path() {
        let sender_keypair = test_keypair();
        let receiver_keypair = test_keypair();
        let mut sender = PTManager::new(sender_keypair);
        let mut receiver = PTManager::new(receiver_keypair);
        let v6_path: SocketAddr = "[2600:100f::1]:4383".parse().unwrap();
        let v4_path: SocketAddr = "192.168.1.156:4383".parse().unwrap();

        let spec_bytes = sender.send(v6_path, vec![0xAB; 1500]);
        let spec_fields = parse_vsf_section_fields(&spec_bytes);
        let spec = PTSpec::from_vsf_fields(&spec_fields).unwrap();
        // Receiver keys the transfer by the SPEC's source (the v6 path)...
        let _ = receiver.handle_spec(v6_path, spec.clone(), None);
        let data_packets = sender.handle_spec_ack(v6_path, spec.stream_id, spec.data_hash);
        // ...but the DATA arrives from the v4 path.
        let first = PTData::from_bytes(&data_packets[0]).unwrap();
        let ack = receiver.handle_data(v4_path, first);
        assert!(ack.is_some(), "unique-stream DATA from a migrated path must be accepted, not dropped");
        assert!(
            receiver.inbound.iter().any(|t| same_addr(t.peer_addr, v4_path)),
            "the transfer adopted the live source address"
        );
    }

    /// SPEC-ACK from a third address (receiver egress ≠ our aim): with an unambiguous stream, the transfer locks onto the answering path instead of ignoring the ACK.
    #[test]
    fn spec_ack_from_third_address_locks_the_answering_path() {
        let mut sender = PTManager::new(test_keypair());
        let aimed: SocketAddr = "[2600:100f::9]:4383".parse().unwrap();
        let answering: SocketAddr = "192.168.1.161:4383".parse().unwrap();
        let spec_bytes = sender.send(aimed, vec![0xCD; 5000]);
        let spec_fields = parse_vsf_section_fields(&spec_bytes);
        let spec = PTSpec::from_vsf_fields(&spec_fields).unwrap();
        let packets = sender.handle_spec_ack(answering, spec.stream_id, spec.data_hash);
        assert!(!packets.is_empty(), "the ACK must start the DATA phase");
        assert!(
            same_addr(sender.outbound[0].peer_addr, answering),
            "transfer locked onto the path that actually answered"
        );
        assert!(sender.outbound[0].spec_acked);
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
        let spec_ack = receiver.handle_spec(peer_addr, spec.clone(), None);
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

    /// The window: thirteen large transfers live per peer, the rest wait; a completion swept by tick releases the next in FIFO order with the peer's next stream id, and ids rotate a..z by modulo per peer.
    #[test]
    fn window_holds_half_the_alphabet_and_releases_on_the_completion_edge() {
        let mut mgr = PTManager::new(test_keypair());
        let peer: SocketAddr = "192.168.1.161:4383".parse().unwrap();
        let pk = [0x5Au8; 32];
        let mut specs = 0;
        for i in 0..40u8 {
            if !mgr.send_with_pubkey(peer, vec![i; 3000], Some(pk)).is_empty() {
                specs += 1;
            }
        }
        assert_eq!(specs, PTManager::STREAMS_IN_FLIGHT, "exactly the window's worth of SPECs go out at once");
        assert_eq!(mgr.outbound.len(), PTManager::STREAMS_IN_FLIGHT);
        assert_eq!(mgr.pending_outbound.len(), 40 - PTManager::STREAMS_IN_FLIGHT);
        let ids: Vec<u8> = mgr.outbound.iter().map(|t| t.stream_id).collect();
        assert_eq!(ids, (0..13u8).map(|n| b'a' + n).collect::<Vec<_>>(), "a..m in flight");
        // Nothing releases while the window is full.
        assert!(mgr.tick().is_empty());
        // Three complete → three released, with the next ids n, o, p.
        for t in mgr.outbound.iter_mut().take(3) {
            t.state = TransferState::Complete;
        }
        let released = mgr.tick();
        assert_eq!(released.len(), 3);
        assert_eq!(mgr.outbound.len(), PTManager::STREAMS_IN_FLIGHT, "swept three, released three");
        assert_eq!(mgr.pending_outbound.len(), 40 - PTManager::STREAMS_IN_FLIGHT - 3);
        let newest: Vec<u8> = mgr.outbound.iter().rev().take(3).map(|t| t.stream_id).collect();
        assert_eq!(newest, vec![b'p', b'o', b'n']);
        // The released transfer carries the first parked payload (FIFO), not the last.
        assert_eq!(mgr.outbound[PTManager::STREAMS_IN_FLIGHT - 3].original_payload.as_ref().unwrap()[0], 13);
        // Rotation: after twenty-six allocations the id comes back to 'a'.
        for _ in 0..40 {
            for t in mgr.outbound.iter_mut() {
                t.state = TransferState::Complete;
            }
            mgr.tick();
        }
        assert!(mgr.pending_outbound.is_empty(), "everything released");
        assert_eq!(mgr.allocate_stream_id(peer_key(Some(pk), peer)), b'a' + (40 % 26) as u8);
        // A different peer has its own alphabet.
        let other: SocketAddr = "10.0.0.9:4383".parse().unwrap();
        assert_eq!(mgr.allocate_stream_id(peer_key(Some([0x11u8; 32]), other)), b'a');
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
        assert!(manager
            .outbound
            .iter()
            .all(|t| same_addr(t.peer_addr, peer_addr)));

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
        mgr.handle_spec(peer, spec(b'a', &data_a), None);
        mgr.handle_spec(peer, spec(b'b', &data_b), None);

        // Deliver both final packets (order intentionally b-then-a to prove drain isn't positional).
        mgr.handle_data(
            peer,
            PTData {
                stream_id: b'b',
                sequence: 0,
                payload: data_b.clone(),
            },
        );
        mgr.handle_data(
            peer,
            PTData {
                stream_id: b'a',
                sequence: 0,
                payload: data_a.clone(),
            },
        );

        // Drain by stream — each must yield ITS OWN payload, not whichever is first in the vec.
        assert!(mgr.check_inbound_complete(peer, b'a').is_some());
        assert!(mgr.check_inbound_complete(peer, b'b').is_some());
        assert_eq!(mgr.take_inbound_data(peer, b'a'), Some(data_a));
        assert_eq!(mgr.take_inbound_data(peer, b'b'), Some(data_b));
    }

    /// A two-packet SPEC on stream 'c' for `d`, and its DATA chunk `seq`.
    fn spec_c(d: &[u8]) -> PTSpec {
        PTSpec {
            stream_id: b'c',
            total_packets: 2,
            packet_size: 1024,
            total_size: d.len() as u32,
            data_hash: *blake3::hash(d).as_bytes(),
        }
    }
    fn chunk_c(d: &[u8], seq: u32) -> PTData {
        let start = seq as usize * 1024;
        PTData { stream_id: b'c', sequence: seq, payload: d[start..d.len().min(start + 1024)].to_vec() }
    }

    /// 2026-10-21 Emma: several devices of one friend each rotate their own a..z, so one letter is live from two devices at once.
    /// Device B's straggler DATA (its own 'c' already landed and drained) must never be adopted into device A's fresh 'c': B signed SPECs from that address, so the address is B's.
    #[test]
    fn another_devices_data_never_joins_a_transfer_on_the_same_letter() {
        let mut rx = PTManager::new(test_keypair());
        let addr_a: SocketAddr = "[::ffff:192.168.1.156]:4383".parse().unwrap();
        let addr_b: SocketAddr = "[::ffff:192.168.1.161]:4383".parse().unwrap();
        let (dev_a, dev_b) = ([0xA1u8; 32], [0xB2u8; 32]);
        let data_a = vec![0xAA; 2000];
        let data_b = vec![0xBB; 2000];

        // B's 'c' lands whole and drains.
        rx.handle_spec(addr_b, spec_c(&data_b), Some(dev_b));
        rx.handle_data(addr_b, chunk_c(&data_b, 0));
        rx.handle_data(addr_b, chunk_c(&data_b, 1));
        assert!(rx.check_inbound_complete(addr_b, b'c').is_some());
        assert_eq!(rx.take_inbound_data(addr_b, b'c'), Some(data_b.clone()));

        // A opens its own 'c'; B's late duplicate arrives before any of A's DATA.
        rx.handle_spec(addr_a, spec_c(&data_a), Some(dev_a));
        // B's straggler is not accepted into A's transfer: B's own transfer was answered, so it draws B's COMPLETE again.
        let reply = rx.handle_data(addr_b, chunk_c(&data_b, 0)).expect("B's straggler draws B's own COMPLETE");
        let (provenance, values) = parse_pt_header_field(&reply).unwrap();
        assert_eq!(PTComplete::from_vsf_header(provenance, &values).unwrap().final_hash, *blake3::hash(&data_b).as_bytes());
        assert!(same_addr(rx.inbound[0].peer_addr, addr_a), "A's transfer keeps A's address");
        assert_eq!(rx.inbound[0].progress().0, 0, "nothing of B's landed in A's buffer");

        // A's own DATA completes and verifies.
        rx.handle_data(addr_a, chunk_c(&data_a, 0));
        rx.handle_data(addr_a, chunk_c(&data_a, 1));
        assert!(rx.check_inbound_complete(addr_a, b'c').is_some());
        assert_eq!(rx.take_inbound_data(addr_a, b'c'), Some(data_a));
    }

    /// The same device speaking from a second address is still one sender: its DATA follows the transfer there even after data has landed, because the address is attributed to it by a signed SPEC.
    #[test]
    fn the_same_device_still_migrates_mid_transfer() {
        let mut rx = PTManager::new(test_keypair());
        let v6: SocketAddr = "[2600:100f::1]:4383".parse().unwrap();
        let v4: SocketAddr = "192.168.1.156:4383".parse().unwrap();
        let dev = [0xA1u8; 32];
        let data = vec![0xAA; 2000];
        let other = vec![0xEE; 64];
        // The device once signed a SPEC from v4 too (another stream), so v4 is known to be it.
        rx.handle_spec(v4, PTSpec { stream_id: b'q', total_packets: 1, packet_size: 1024, total_size: 64, data_hash: *blake3::hash(&other).as_bytes() }, Some(dev));
        rx.handle_spec(v6, spec_c(&data), Some(dev));
        assert!(rx.handle_data(v6, chunk_c(&data, 0)).is_some());
        assert!(rx.handle_data(v4, chunk_c(&data, 1)).is_some(), "the same device's DATA from its other address is adopted");
        assert!(rx.check_inbound_complete(v4, b'c').is_some());
        assert_eq!(rx.take_inbound_data(v4, b'c'), Some(data));
    }

    /// A bad assembly answers once, naming the promised hash so the sender can retire its transfer, and is then retired itself: no re-verify and no second COMPLETE per straggler.
    #[test]
    fn a_hash_mismatch_answers_once_and_frees_the_sender() {
        let mut rx = PTManager::new(test_keypair());
        let peer: SocketAddr = "192.168.1.156:4383".parse().unwrap();
        let data = vec![0xAA; 2000];
        rx.handle_spec(peer, spec_c(&data), None);
        rx.handle_data(peer, chunk_c(&data, 0));
        rx.handle_data(peer, chunk_c(&vec![0xBB; 2000], 1));
        let complete_bytes = rx.check_inbound_complete(peer, b'c').expect("a complete assembly answers");
        let (provenance, values) = parse_pt_header_field(&complete_bytes).unwrap();
        let complete = PTComplete::from_vsf_header(provenance, &values).unwrap();
        assert!(!complete.success);
        assert_eq!(complete.final_hash, *blake3::hash(&data).as_bytes(), "the failure names the hash the sender promised");
        assert!(rx.check_inbound_complete(peer, b'c').is_none(), "answered once, then retired");
        rx.tick();
        assert!(rx.inbound.is_empty(), "the tick sweeps the retired assembly");
    }

    /// 2026-10-21: the receiver's COMPLETE is lost on the wire, and the sender's transfer used to hold one of the peer's window slots forever.
    /// The chunk that finished the set stays in flight as a probe; the receiver, which drained the transfer long ago, answers the probe with the same COMPLETE, and the slot comes free.
    #[test]
    fn a_lost_complete_is_answered_again_when_the_sender_probes() {
        let mut sender = PTManager::new(test_keypair());
        let mut receiver = PTManager::new(test_keypair());
        let peer: SocketAddr = "192.168.1.156:4383".parse().unwrap();
        let pk = [0x5Au8; 32];
        let data = vec![0xAB; 3000];

        let spec_bytes = sender.send_with_pubkey(peer, data.clone(), Some(pk));
        let spec = PTSpec::from_vsf_fields(&parse_vsf_section_fields(&spec_bytes)).unwrap();
        receiver.handle_spec(peer, spec.clone(), Some([0xA1; 32]));
        let mut wire = sender.handle_spec_ack(peer, spec.stream_id, spec.data_hash);
        let mut answered = false;
        while !wire.is_empty() {
            let mut next = Vec::new();
            for bytes in wire {
                let ack_bytes = receiver.handle_data(peer, PTData::from_bytes(&bytes).unwrap()).unwrap();
                // The COMPLETE goes out and is lost; the receiver drains the transfer as it always does.
                if receiver.check_inbound_complete(peer, spec.stream_id).is_some() {
                    answered = true;
                    assert_eq!(receiver.take_inbound_data(peer, spec.stream_id), Some(data.clone()));
                }
                let (provenance, values) = parse_pt_header_field(&ack_bytes).unwrap();
                next.extend(sender.handle_ack(peer, PTAck::from_vsf_header(provenance, &values).unwrap()));
            }
            wire = next;
        }
        assert!(answered && receiver.inbound.is_empty());
        assert_eq!(sender.outbound[0].state, TransferState::AwaitingComplete);
        assert_eq!(sender.outbound[0].flight.count(), 1, "the chunk that finished the set stays in flight as the probe");
        // A repeat SPEC ACK must not pull it back to Transferring.
        assert!(sender.handle_spec_ack(peer, spec.stream_id, spec.data_hash).is_empty());
        assert_eq!(sender.outbound[0].state, TransferState::AwaitingComplete);
        sender.tick();
        assert_eq!(sender.in_flight_for(peer_key(Some(pk), peer)), 1, "still waiting, not retired: the ladder has budget left");

        // The probe (what the ladder re-sends; built here because the ladder's timing is not under test) draws the COMPLETE again.
        let last = sender.outbound[0].send_buffer.total_packets() - 1;
        let probe = PTData { stream_id: spec.stream_id, sequence: last, payload: sender.outbound[0].send_buffer.get_packet(last).unwrap().to_vec() };
        let answer = receiver.handle_data(peer, probe).expect("an answered transfer's chunk draws its COMPLETE");
        let (provenance, values) = parse_pt_header_field(&answer).unwrap();
        let complete = PTComplete::from_vsf_header(provenance, &values).unwrap();
        assert!(complete.success);
        assert_eq!(complete.final_hash, spec.data_hash);
        sender.handle_complete(peer, complete);
        sender.tick();
        assert!(sender.outbound.is_empty(), "completed and swept");
        assert_eq!(sender.in_flight_for(peer_key(Some(pk), peer)), 0, "the window slot is free");

        // A receiver that has forgotten (restarted) never answers: when the ladder runs out the transfer retires instead of waiting forever.
        let _ = sender.send_with_pubkey(peer, vec![0xCD; 3000], Some(pk));
        sender.outbound[0].state = TransferState::AwaitingComplete;
        sender.outbound[0].retries = 11; // forced: the ladder's timing is not under test
        sender.tick();
        assert!(sender.outbound.is_empty(), "an unanswered probe ladder frees the slot");

        // The same sender reusing the letter for other bytes retires the old answer.
        let other = vec![0xEE; 64];
        receiver.handle_spec(peer, PTSpec { stream_id: spec.stream_id, total_packets: 1, packet_size: 1024, total_size: 64, data_hash: *blake3::hash(&other).as_bytes() }, Some([0xA1; 32]));
        assert!(receiver.answered.is_empty());
    }

    /// 2026-10-20 Nick: an expired temporary IPv6 took 548 give-ups in a row.
    /// The first exhausted head kills the address and its queue; new sends are refused; the first datagram heard from it reopens it.
    #[test]
    fn an_address_that_never_answers_is_dead_until_heard_from() {
        let mut mgr = PTManager::new(test_keypair());
        let dead: SocketAddr = "[2600:100f:a020:deec:d048:3f71:19f3:9e3a]:4383".parse().unwrap();
        let live: SocketAddr = "192.168.1.159:4383".parse().unwrap();
        assert!(!mgr.send(dead, vec![1; 64]).is_empty());
        for i in 2..6u8 {
            let _ = mgr.send(dead, vec![i; 64]);
        }
        // One queued packet names a live alternate: it must be re-aimed, not dropped.
        let _ = mgr.send_with_pubkey_and_alt(dead, Some(live), vec![7; 64], None);
        // A retransmit of the same bytes stacks nothing.
        let _ = mgr.send(dead, vec![2; 64]);
        assert_eq!(mgr.outbound_packets.len(), 6);
        // The head exhausts its ladder (forced: the ladder's timing is not under test).
        mgr.outbound_packets[0].retry_count = PTManager::MAX_PACKET_RETRIES;
        let sent = mgr.tick();
        assert_eq!(mgr.outbound_packets.len(), 1, "the queue behind the dead address went with its head, all but the re-aimed one");
        assert!(same_addr(mgr.outbound_packets[0].peer_addr, live) && mgr.outbound_packets[0].in_flight);
        assert!(sent.iter().any(|s| same_addr(s.peer_addr, live)), "the re-aimed packet went out on the live path");
        assert!(mgr.send(dead, vec![9; 64]).is_empty());
        assert_eq!(mgr.outbound_packets.len(), 1, "new sends to the dead address are refused, not stacked");
        // The revival edge is the dead address's own datagram; anyone else's revives nothing.
        mgr.heard_from(live);
        assert!(mgr.send(dead, vec![9; 64]).is_empty(), "an unrelated datagram revives nothing");
        mgr.heard_from(dead);
        assert!(!mgr.send(dead, vec![9; 64]).is_empty(), "heard from again: sends resume");
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
