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

/// Classify a peer public address into a candidate kind: a v6 public address is a direct host (no NAT rewriting v6, so no punch needed); a v4 public address is reached by hole-punch → reflexive-class.
fn public_kind(addr: &SocketAddr) -> CandidateKind {
    if addr.is_ipv6() {
        CandidateKind::HostV6
    } else {
        CandidateKind::Reflexive
    }
}

/// True for an address that must NEVER enter the candidate set: the unspecified `0.0.0.0` / `::` — which is the RELAY_ADDR sentinel a relayed message carries. If it leaks in, the punch "validates" a path to `0.0.0.0` (it round-trips locally), which then poisons all addressing: sends go to nowhere and `relay_to` empties out because `validated_path` looks Some. Seen live as mom's proof vanishing after `path validated to … = [0,0,0,0]`.
fn is_bogus_addr(addr: &SocketAddr) -> bool {
    addr.ip().is_unspecified()
}

/// The addresses at which `contact` might be reachable — their public address (reflexive, or a v6 host), their usable LAN address, and every per-device endpoint we've learned. This is the set we punch toward and (via [`CandidateSet::best_pair`]) the send order. Scanning `device_endpoints`, not just the active `ip`, is what surfaces a peer's global IPv6 when the active address happens to be v4 (e.g. a device that ponged over v6 while the phonebook only carried its v4 WAN) — so the v6 host, priority-first, gets tried before a v4 LAN address that may be on a foreign network.
pub fn gather_peer_candidates(contact: &Contact) -> CandidateSet {
    let mut set = CandidateSet::new();

    if let Some(ip) = contact.ip {
        if !is_bogus_addr(&ip) {
            set.add(Candidate::new(ip, public_kind(&ip)));
        }
    }

    if let (Some(local_v4), Some(port)) = (contact.local_ip, contact.local_port) {
        // Skip an unreachable LAN candidate (464XLAT CLAT `192.0.0.4` and friends) — same filter race_addrs uses.
        if crate::network::udp::is_usable_lan_ipv4(local_v4) {
            let lan = SocketAddr::new(IpAddr::V4(local_v4), port);
            set.add(Candidate::new(lan, CandidateKind::HostV4Lan));
        }
    }

    // Every device's learned endpoints — a sibling reachable over v6 becomes a HostV6 candidate even when the active `ip` is a v4 WAN address. `add` dedups by address and keeps the higher-priority kind.
    for ep in &contact.device_endpoints {
        if let Some(pub_addr) = ep.public {
            if !is_bogus_addr(&pub_addr) {
                set.add(Candidate::new(pub_addr, public_kind(&pub_addr)));
            }
        }
        if let Some(lan_addr) = ep.lan {
            if let IpAddr::V4(v4) = lan_addr.ip() {
                if crate::network::udp::is_usable_lan_ipv4(v4) {
                    set.add(Candidate::new(lan_addr, CandidateKind::HostV4Lan));
                }
            }
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
