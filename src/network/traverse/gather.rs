//! Candidate gathering — turning known addresses into a [`CandidateSet`].
//!
//! Two directions:
//! - [`gather_peer_candidates`] builds the set of addresses at which a *peer* might be reachable (where we send probes), from what we already know about them: their public address and their LAN address. This reads the same `Contact` fields `race_addrs` does, so [`CandidateSet::best_pair`] reproduces its result.
//! - [`gather_own_candidates`] builds the set of *our* addresses to advertise to a peer so they can punch back at us: our learned reflexive address and our own LAN address.
//!
//! Full local-interface enumeration (multiple NICs, a global-IPv6 host address) is deferred to when the candidate offer actually ships (P2); for now our own set is reflexive + the one LAN v4 the OS routes on.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use super::candidate::{Candidate, CandidateKind, CandidateSet};
use crate::types::contact::Contact;

/// The addresses at which `contact` might be reachable — their public address (reflexive, or a v6 host) and their usable LAN address. This is the set we punch toward.
pub fn gather_peer_candidates(contact: &Contact) -> CandidateSet {
    let mut set = CandidateSet::new();

    if let Some(ip) = contact.ip {
        // Their announced public address. A v6 public address is a direct host (no NAT rewriting v6); a v4 public address is reached by hole-punch → reflexive-class.
        let kind = if ip.is_ipv6() {
            CandidateKind::HostV6
        } else {
            CandidateKind::Reflexive
        };
        set.add(Candidate::new(ip, kind));
    }

    if let (Some(local_v4), Some(port)) = (contact.local_ip, contact.local_port) {
        // Skip an unreachable LAN candidate (464XLAT CLAT `192.0.0.4` and friends) — same filter race_addrs uses.
        if crate::network::udp::is_usable_lan_ipv4(local_v4) {
            let lan = SocketAddr::new(IpAddr::V4(local_v4), port);
            set.add(Candidate::new(lan, CandidateKind::HostV4Lan));
        }
    }

    set
}

/// Our own addresses to advertise so a peer can punch back at us: our learned reflexive address (public, from peer-echoed reflection) and our LAN address on the port we listen on.
pub fn gather_own_candidates(
    our_reflexive: Option<SocketAddr>,
    local_v4: Option<Ipv4Addr>,
    port: u16,
) -> CandidateSet {
    let mut set = CandidateSet::new();

    if let Some(refl) = our_reflexive {
        let kind = if refl.is_ipv6() {
            CandidateKind::HostV6
        } else {
            CandidateKind::Reflexive
        };
        set.add(Candidate::new(refl, kind));
    }

    if let Some(v4) = local_v4 {
        if crate::network::udp::is_usable_lan_ipv4(v4) {
            set.add(Candidate::new(
                SocketAddr::new(IpAddr::V4(v4), port),
                CandidateKind::HostV4Lan,
            ));
        }
    }

    set
}
