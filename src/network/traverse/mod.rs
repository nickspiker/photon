//! NAT traversal — turning a peer identity into a working socket address.
//!
//! Photon's transport (PT) assumes it already has a reachable `SocketAddr`; this module is what produces one. It assembles the pieces photon already had (a dual-stack socket, signed contact-gated ping/pong, PT's address racing) into a real connection-establishment handshake: gather candidates → learn our reflexive address → exchange candidates → coordinated simultaneous hole-punch → validate a working path → hand it to PT. See the traversal plan for the milestone breakdown.
//!
//! Two trust tiers run thru here:
//! - **Friend tier** (data plane): punch-for-delivery, contact/fleet-gated exactly like ping.
//! - **Directory tier** (open substrate): address reflection + phonebook serving, open to any node under the "serve directory" setting, safe because trustless (self-signed records, reflection reveals only the requester's own address).
//!
//! # Where this code lives now
//!
//! The transport-agnostic half lives in the `fgtw` crate's `traverse` module so that photon and rustdesk share one implementation instead of two that drift. `candidate`, `reflexive` and
//! `session` are straight re-exports; [`gather`] stays local because it adapts photon's
//! `Contact` onto the crate's endpoint shape; [`punch`] stays local for now because it encodes into `FgtwMessage`.
//!
//! Photon drives the crate's state machines from its own receive loop in `network::status`,
//! which multiplexes one socket across the whole data plane. It therefore uses the state machines but NOT the crate's `driver` module — rustdesk, which has no such loop, uses the driver instead. That asymmetry is deliberate; see the crate docs before "fixing" it.

pub mod gather;
pub mod portmap;
pub mod punch;

pub use fgtw::traverse::{candidate, reflexive, session};

/// Raised on the UI thread when our LAN address CHANGES (a network we have left); taken by the status receive loop before its next reflexive observation, which then starts from nothing (2026-09-11: with public-beats-private in place, a stale public address would otherwise outlive the network it belonged to).
static REFLEXIVE_RESET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn request_reflexive_reset() {
    REFLEXIVE_RESET.store(true, std::sync::atomic::Ordering::Release);
}

pub fn take_reflexive_reset() -> bool {
    REFLEXIVE_RESET.swap(false, std::sync::atomic::Ordering::AcqRel)
}

/// THE FGTW-OBSERVED ADDRESS AFTER A NETWORK CHANGE (field 2026-09-14, Nick on cellular: the interface change forgot the reflexive, no peer could reach him to reflect a new one, and his push carried the carrier-NAT interface address — the wave never found a path). A worker re-announces to FGTW over TLS (that works from anywhere) and parks the server's observation here; the UI tick seeds `our_reflexive` from it and pushes the record to the live wave's peer.
static REFLEXIVE_SEED: std::sync::Mutex<Option<std::net::SocketAddr>> = std::sync::Mutex::new(None);

/// LAN-scope: an address only a same-network observer can report (RFC 1918 + 6598 v4, ULA/link-local v6). Never the address the internet reaches us at.
pub fn is_lan_scope(addr: &std::net::SocketAddr) -> bool {
    match addr.ip().to_canonical() {
        std::net::IpAddr::V4(v4) => gather::is_private_ipv4(v4) || v4.is_link_local() || v4.is_loopback(),
        std::net::IpAddr::V6(v6) => v6.is_unique_local() || v6.is_unicast_link_local() || v6.is_loopback(),
    }
}

pub fn post_reflexive_seed(addr: std::net::SocketAddr) {
    *REFLEXIVE_SEED.lock().unwrap() = Some(addr);
}

pub fn take_reflexive_seed() -> Option<std::net::SocketAddr> {
    REFLEXIVE_SEED.lock().unwrap().take()
}
