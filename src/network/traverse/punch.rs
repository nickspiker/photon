//! Hole-punch probe/ack helpers and the pending-probe tracker.
//!
//! A probe is a signed datagram fired at a peer's candidate address: *sending* it opens our NAT toward that
//! address, and a friend's ack (echoing the probe's provenance) confirms the `(local, remote)` pair
//! round-trips → that path is validated. Probes are tracked by their provenance hash so an incoming ack
//! identifies which candidate won, for which peer.
//!
//! The probe is friend-tier: a friend answers it (contact/fleet-gated in the dispatch, exactly like ping),
//! a stranger is ignored. Wire framing is the canonical full-header VSF via [`FgtwMessage::to_vsf_bytes`].

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use crate::network::fgtw::protocol::FgtwMessage;
use crate::network::fgtw::Keypair;
use crate::types::device::DevicePubkey;

/// A probe is abandoned if unanswered this long (a candidate that never round-trips).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Build a signed hole-punch probe. Returns the wire bytes and the provenance hash (the per-probe
/// correlator the ack echoes back). `nonce` guarantees the provenance is unique even within one time tick.
pub fn build_probe(
    keypair: &Keypair,
    sender_pubkey: DevicePubkey,
    nonce: [u8; 32],
) -> (Vec<u8>, [u8; 32]) {
    let timestamp = vsf::eagle_time_oscillations();
    let mut h = blake3::Hasher::new();
    h.update(sender_pubkey.as_bytes());
    h.update(&timestamp.to_le_bytes());
    h.update(&nonce);
    let provenance_hash: [u8; 32] = *h.finalize().as_bytes();

    let sig = keypair.sign(&provenance_hash);
    let mut signature = [0u8; 64];
    signature.copy_from_slice(&sig.to_bytes());

    let msg = FgtwMessage::PunchProbe {
        timestamp,
        sender_pubkey,
        provenance_hash,
        signature,
    };
    (msg.to_vsf_bytes(), provenance_hash)
}

/// Build a signed ack echoing `provenance_hash` and reporting the address we observed the probe arrive from
/// (canonicalise it with [`crate::network::udp::canon_socketaddr`] before calling).
pub fn build_probe_ack(
    keypair: &Keypair,
    responder_pubkey: DevicePubkey,
    provenance_hash: [u8; 32],
    observed_addr: SocketAddr,
) -> Vec<u8> {
    let sig = keypair.sign(&provenance_hash);
    let mut signature = [0u8; 64];
    signature.copy_from_slice(&sig.to_bytes());

    FgtwMessage::PunchProbeAck {
        timestamp: vsf::eagle_time_oscillations(),
        responder_pubkey,
        provenance_hash,
        signature,
        observed_addr,
    }
    .to_vsf_bytes()
}

/// Outstanding probes: an ack matched by provenance tells us which candidate validated, for which peer.
#[derive(Default)]
pub struct PendingProbes {
    inner: HashMap<[u8; 32], PendingProbe>,
}

struct PendingProbe {
    peer: DevicePubkey,
    target: SocketAddr,
    sent_at: Instant,
}

impl PendingProbes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, provenance: [u8; 32], peer: DevicePubkey, target: SocketAddr, now: Instant) {
        self.inner.insert(
            provenance,
            PendingProbe {
                peer,
                target,
                sent_at: now,
            },
        );
    }

    /// Resolve an ack: if we sent this probe, remove and return `(peer, target)`; `None` for an
    /// unknown or duplicate ack (so a replayed ack can't re-validate a stale path).
    pub fn resolve(&mut self, provenance: &[u8; 32]) -> Option<(DevicePubkey, SocketAddr)> {
        self.inner.remove(provenance).map(|p| (p.peer, p.target))
    }

    /// Drop probes older than [`PROBE_TIMEOUT`]; returns how many expired (a candidate that never answered).
    pub fn expire(&mut self, now: Instant) -> usize {
        let before = self.inner.len();
        self.inner
            .retain(|_, p| now.duration_since(p.sent_at) < PROBE_TIMEOUT);
        before - self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn punch_probe_codec_roundtrips_full_header() {
        // The wire format is the risky part — verify a probe (body-less) and an ack (with obs) round-trip
        // through the canonical full-header VSF path, independent of signing.
        let pk = DevicePubkey::from_bytes([7u8; 32]);
        let prov = [9u8; 32];

        let probe = FgtwMessage::PunchProbe {
            timestamp: 12345,
            sender_pubkey: pk.clone(),
            provenance_hash: prov,
            signature: [0u8; 64],
        };
        let bytes = probe.to_vsf_bytes();
        assert!(bytes.starts_with(b"R\xC3\x85"), "must be full VSF file with magic");
        match FgtwMessage::from_vsf_bytes(&bytes).expect("parse probe") {
            FgtwMessage::PunchProbe {
                provenance_hash, ..
            } => assert_eq!(provenance_hash, prov),
            _ => panic!("wrong variant for probe"),
        }

        let obs = addr("203.0.113.9:4383");
        let ack = FgtwMessage::PunchProbeAck {
            timestamp: 12345,
            responder_pubkey: pk,
            provenance_hash: prov,
            signature: [0u8; 64],
            observed_addr: obs,
        };
        let bytes = ack.to_vsf_bytes();
        match FgtwMessage::from_vsf_bytes(&bytes).expect("parse ack") {
            FgtwMessage::PunchProbeAck {
                provenance_hash,
                observed_addr,
                ..
            } => {
                assert_eq!(provenance_hash, prov);
                assert_eq!(observed_addr, obs);
            }
            _ => panic!("wrong variant for ack"),
        }
    }

    #[test]
    fn pending_probe_resolves_once() {
        let mut p = PendingProbes::new();
        let now = Instant::now();
        let peer = DevicePubkey::from_bytes([1u8; 32]);
        let target = addr("192.168.1.5:4383");
        p.insert([42u8; 32], peer.clone(), target, now);

        let resolved = p.resolve(&[42u8; 32]);
        assert!(resolved.is_some());
        let (rp, rt) = resolved.unwrap();
        assert!(rp == peer);
        assert_eq!(rt, target);

        assert!(p.resolve(&[42u8; 32]).is_none()); // duplicate/replayed ack does not re-validate
    }

    #[test]
    fn pending_probe_expires() {
        let mut p = PendingProbes::new();
        let past = Instant::now() - (PROBE_TIMEOUT + Duration::from_secs(1));
        p.insert([1u8; 32], DevicePubkey::from_bytes([1u8; 32]), addr("1.2.3.4:1"), past);
        assert_eq!(p.expire(Instant::now()), 1);
        assert!(p.is_empty());
    }
}
