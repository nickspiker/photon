//! Per-peer reachability state — the result of traversal, cached with freshness so a warm path is reused
//! without re-punching, and a stale one triggers a re-punch or falls to relay (M2).

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use super::candidate::CandidateSet;
use crate::types::device::DevicePubkey;

/// How long a validated path is trusted before it must be re-validated.
pub const PATH_TTL: Duration = Duration::from_secs(120);
/// Idle keepalive interval — kept below the common 30–60s UDP NAT-mapping timeout so the hole stays open.
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(20);

/// The outcome of a punch attempt for one peer.
#[derive(Debug, Clone)]
pub enum PunchState {
    /// No attempt yet.
    Idle,
    /// Gathering candidates / advertising ours, before probes fly.
    Gathering,
    /// Probing candidates, awaiting the first ack.
    Punching { started: Instant, retransmits: u8 },
    /// A working remote address, validated by a probe round-trip.
    Validated(SocketAddr),
    /// No direct path exists (e.g. symmetric↔symmetric). The relay milestone (M2) attaches here.
    Unreachable(Unreachable),
}

/// Why a peer is unreachable directly, and the hook M2 reads.
#[derive(Debug, Clone, Copy)]
pub struct Unreachable {
    /// A relay could still connect these two — set once direct punching is exhausted. M2's relay path activates on this.
    pub pending_relay: bool,
}

/// A validated direct path to a peer, with freshness tracking for keepalive/expiry.
#[derive(Debug, Clone, Copy)]
pub struct ValidatedPath {
    pub remote: SocketAddr,
    pub validated_at: Instant,
    pub last_rx: Instant,
}

impl ValidatedPath {
    pub fn new(remote: SocketAddr, now: Instant) -> Self {
        Self {
            remote,
            validated_at: now,
            last_rx: now,
        }
    }

    /// Fresh enough to use without re-punching.
    pub fn is_still_valid(&self, now: Instant) -> bool {
        now.duration_since(self.validated_at) < PATH_TTL
    }

    /// Time to send an idle keepalive to hold the NAT mapping open (only when nothing else has been received recently).
    pub fn keepalive_due(&self, now: Instant) -> bool {
        now.duration_since(self.last_rx) >= KEEPALIVE_INTERVAL
    }
}

/// Per-peer traversal state, keyed in the orchestrator by device pubkey.
pub struct PeerSession {
    pub pubkey: DevicePubkey,
    pub state: PunchState,
    pub validated: Option<ValidatedPath>,
    pub candidates: CandidateSet,
}

impl PeerSession {
    pub fn new(pubkey: DevicePubkey) -> Self {
        Self {
            pubkey,
            state: PunchState::Idle,
            validated: None,
            candidates: CandidateSet::new(),
        }
    }

    /// The validated remote address, if we have one that's still fresh.
    pub fn cached_remote(&self, now: Instant) -> Option<SocketAddr> {
        self.validated
            .filter(|v| v.is_still_valid(now))
            .map(|v| v.remote)
    }
}
