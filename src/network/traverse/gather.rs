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

/// True for an address that must NEVER enter the candidate set: the unspecified `0.0.0.0` / `::` — which is the RELAY_ADDR sentinel a relayed message carries. If it leaks in, the punch "validates" a path to `0.0.0.0` (it round-trips locally), which then poisons all addressing: sends go to nowhere and `relay_to` empties out because `validated_path` looks Some. Observed as a peer's proof vanishing after a spurious `path validated to … = [0,0,0,0]`.
fn is_bogus_addr(addr: &SocketAddr) -> bool {
    addr.ip().is_unspecified()
}

/// Would a peer's private IPv4 `peer_v4` plausibly be reachable from us, given OUR own LAN v4 `our_v4`?
/// A peer's private range (`192.168/16`, `10/8`, `172.16/12`) is only reachable when we share its subnet — otherwise it is a FOREIGN LAN address that PT retransmits into a black hole, wasting the direct-path budget and masking that the relay is the real path.
/// This is common in practice because default home routers all hand out the same `192.168.0.*` block, so two unrelated peers routinely carry colliding-but-unreachable private addresses.
/// We approximate "same subnet" as a shared `/24` — the common home-LAN mask; a wider real mask only makes us slightly conservative (fall back to public/relay), never sending to an unreachable address.
/// With no known LAN of our own (`our_v4 == None`) we can't vouch for any peer LAN, so we exclude it — the public + v6 + relay paths still carry the peer.
fn peer_lan_reachable(peer_v4: std::net::Ipv4Addr, our_v4: Option<std::net::Ipv4Addr>) -> bool {
    match our_v4 {
        Some(ours) => {
            let (a, b) = (peer_v4.octets(), ours.octets());
            a[0] == b[0] && a[1] == b[1] && a[2] == b[2]
        }
        None => false,
    }
}

/// The addresses at which `contact` might be reachable — their public address (reflexive, or a v6 host), their usable LAN address, and every per-device endpoint we've learned. This is the set we punch toward and (via [`CandidateSet::best_pair`]) the send order. Scanning `device_endpoints`, not just the active `ip`, is what surfaces a peer's global IPv6 when the active address happens to be v4 (e.g. a device that ponged over v6 while the phonebook only carried its v4 WAN) — so the v6 host, priority-first, gets tried before a v4 LAN address that may be on a foreign network.
///
/// `our_v4` is OUR own LAN IPv4 when we have one.
/// A peer's private-v4 candidate is only added when it shares our `/24` (see [`peer_lan_reachable`]) — a foreign private address from a different network is dropped so we never punch/PT toward a black hole.
/// The convenience [`gather_peer_candidates`] preserves the older subnet-agnostic behaviour for callers with no our-LAN context; send-decision sites that DO know it call this `_from` form so a genuinely same-subnet peer still gets its fast LAN path.
pub fn gather_peer_candidates_from(contact: &Contact, our_v4: Option<std::net::Ipv4Addr>) -> CandidateSet {
    let mut set = CandidateSet::new();

    if let Some(ip) = contact.ip {
        if !is_bogus_addr(&ip) {
            set.add(Candidate::new(ip, public_kind(&ip)));
        }
    }

    if let (Some(local_v4), Some(port)) = (contact.local_ip, contact.local_port) {
        // Skip an unreachable LAN candidate: the 464XLAT CLAT `192.0.0.4` family (is_usable_lan_ipv4), AND a foreign LAN not on our subnet (peer_lan_reachable).
        if crate::network::udp::is_usable_lan_ipv4(local_v4) && peer_lan_reachable(local_v4, our_v4) {
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
                if crate::network::udp::is_usable_lan_ipv4(v4) && peer_lan_reachable(v4, our_v4) {
                    set.add(Candidate::new(lan_addr, CandidateKind::HostV4Lan));
                }
            }
        }
    }

    set
}

/// Convenience wrapper preserving the ORIGINAL subnet-agnostic behaviour for callers with no our-LAN context: a peer LAN candidate is kept as long as the address is a usable LAN v4.
/// This is what `Contact::race_addrs` and the punch-candidate gathers still use — changing THEM would need our-LAN threaded through every send site, a wide and risky change.
/// The foreign-LAN filter is applied instead at the specific send-decision sites that both hold our-LAN and actually black-holed on it (the PT retransmit sweep), via `gather_peer_candidates_from` + `peer_lan_reachable`.
pub fn gather_peer_candidates(contact: &Contact) -> CandidateSet {
    let mut set = CandidateSet::new();

    if let Some(ip) = contact.ip {
        if !is_bogus_addr(&ip) {
            set.add(Candidate::new(ip, public_kind(&ip)));
        }
    }
    if let (Some(local_v4), Some(port)) = (contact.local_ip, contact.local_port) {
        if crate::network::udp::is_usable_lan_ipv4(local_v4) {
            let lan = SocketAddr::new(IpAddr::V4(local_v4), port);
            set.add(Candidate::new(lan, CandidateKind::HostV4Lan));
        }
    }
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

/// True if `peer` is a private IPv4 NOT on our `/24` (a foreign LAN we can't reach) — the exact address a caller with our-LAN should refuse to send to directly.
/// `our_v4 == None` (LAN unknown) means any private peer address is unvouchable, hence foreign.
/// A public/global v4 is never foreign (returns false).
pub fn is_foreign_peer_lan(peer: &SocketAddr, our_v4: Option<std::net::Ipv4Addr>) -> bool {
    match peer.ip() {
        IpAddr::V4(v4) if crate::network::udp::is_private_ipv4(v4) => !peer_lan_reachable(v4, our_v4),
        _ => false,
    }
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
