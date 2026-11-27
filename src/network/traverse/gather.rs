//! Candidate gathering — photon's `Contact` adapter over [`fgtw::traverse::gather`].
//!
//! The gathering logic itself lives in the shared crate so photon and rustdesk can't drift.
//! What stays here is the part that is genuinely photon's: flattening a [`Contact`] — with its active-device address, its per-device endpoint list, and its Wi-Fi Direct group address — into the crate's [`PeerEndpoint`] shape.
//!
//! The two entry points differ only in whether a peer's LAN address must share our `/24`;
//! that is now [`LanPolicy`] rather than two near-identical copies of the same loop.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use crate::types::contact::{is_private_ip, AddrSource, Contact};
use fgtw::traverse::candidate::CandidateSet;
use fgtw::traverse::gather::{LanPolicy, PeerEndpoint};

// Re-exported so the ~23 existing `traverse::gather::…` call sites across photon keep resolving unchanged.
pub use fgtw::traverse::gather::{
    gather_own_candidates, is_bogus_addr, is_foreign_peer_lan, is_private_ipv4, is_usable_lan_ipv4,
    is_wfd_subnet, peer_lan_reachable, public_kind,
};

/// Flatten a contact into the crate's endpoint shape: the active device first (its `ip` plus its `local_ip`/`local_port` pair), then every learned per-device endpoint.
///
/// Scanning all of them, not just the active `ip`, is what surfaces a peer's global IPv6 when the active address happens to be v4 — so the v6 host, priority-first, gets tried before a v4 LAN address that may be on a foreign network.
/// The candidates for ONE device of a contact — a wave is with a device, not with a roster. Aiming media at a peer's OTHER device is dead by construction: it is not in the wave and holds none of its keys (field 2026-09-11, a wave over WAN aimed its recovery probe at the peer's desktop). Empty when we know no endpoint for that device.
pub fn gather_device_candidates(contact: &Contact, device: &[u8; 32]) -> CandidateSet {
    gather_device_candidates_from(contact, device, crate::network::udp::get_local_ip())
}

/// [`gather_device_candidates`] with our own LAN v4 in hand: the peer's private-v4 candidates survive only when they could actually be on our subnet. THE MEDIA PLANE MUST USE THIS (field 2026-09-11: a phone on cellular aimed a wave at the peer's 192.168.1.x, and the peer aimed back at ours — two black holes, zero packets, while the ring read green). A foreign LAN address is not a worse route, it is no route.
pub fn gather_device_candidates_from(contact: &Contact, device: &[u8; 32], our_v4: Option<Ipv4Addr>) -> CandidateSet {
    let eps: Vec<PeerEndpoint> = contact
        .device_endpoints
        .iter()
        .filter(|ep| ep.pubkey == *device)
        .flat_map(endpoint_peers)
        .collect();
    // `contact.ip` / `p2p_addr` are the contact's own first-met pair; they belong to the device we first met, so they only join when that is the device we are asking about.
    let p2p = (contact.device_key() == Some(*device)).then_some(contact.p2p_addr).flatten();
    fgtw::traverse::gather::gather_peer_candidates(&eps, p2p, LanPolicy::SameSubnetAs(our_v4))
}

/// Every address a device is known at that is ours to try, one candidate endpoint each, most recently heard first (the candidate sort is stable, so within a kind the freshest is tried first).
/// A private address is a place on the DEVICE's network: it is tried only when the device was heard or beaconed there by us, or when the device's public IPv4 is ours (behind the same NAT, so on the same LAN).
/// Every default home router hands out the same 192.168.1.x block, so sharing our /24 says nothing (2026-11-26: a friend's 192.168.1.10, on their network, was tried from Nick's desktop 25 times into "No route to host").
/// A public IPv4 that is ours is the outside of our own NAT: the router maps it back to us, never to the device, so it is dropped and the device's LAN addresses are its path (Esme's device on Nick's network, reached only thru the relay).
/// With our public IPv4 not yet learned nothing is judged, and the caller's LAN policy alone applies, as before.
fn endpoint_peers(ep: &crate::types::DeviceEndpoint) -> Vec<PeerEndpoint> {
    peers_judged(ep.addrs.iter().map(|a| (a.addr, matches!(a.how, AddrSource::Heard | AddrSource::Beacon))), crate::network::traverse::our_public_v4())
}

/// The judgement [`endpoint_peers`] makes, over (address, proven from here) pairs most recently heard first, given our public IPv4.
fn peers_judged(addrs: impl Iterator<Item = (SocketAddr, bool)> + Clone, our_public: Option<Ipv4Addr>) -> Vec<PeerEndpoint> {
    let v4_of = |a: &SocketAddr| match a.ip().to_canonical() {
        IpAddr::V4(v) => Some(v),
        IpAddr::V6(_) => None,
    };
    // The device's newest public IPv4 names the NAT it is behind.
    let theirs = addrs.clone().filter(|(a, _)| !is_private_ip(&a.ip())).find_map(|(a, _)| v4_of(&a));
    let elsewhere = our_public.is_some() && theirs.is_some() && theirs != our_public;
    addrs
        .filter_map(|(a, proven)| {
            if is_private_ip(&a.ip()) {
                (!elsewhere || proven).then_some(PeerEndpoint { public: None, lan: Some(a) })
            } else {
                (our_public.is_none() || v4_of(&a) != our_public).then_some(PeerEndpoint { public: Some(a), lan: None })
            }
        })
        .collect()
}

/// Every address of `ep` that is ours to try ([`endpoint_peers`]'s judgement), LAN first, for a sender that walks a device's addresses itself.
pub fn device_addrs(ep: &crate::types::DeviceEndpoint) -> Vec<SocketAddr> {
    let eps = endpoint_peers(ep);
    let lan = eps.iter().filter_map(|p| p.lan);
    let public = eps.iter().filter_map(|p| p.public);
    lan.chain(public).collect()
}

fn contact_endpoints(contact: &Contact) -> Vec<PeerEndpoint> {
    let mut eps = Vec::with_capacity(contact.device_endpoints.len() + 2);
    // The first-met pair judged as one device's addresses (`endpoint_peers`), the LAN half never proven from here: kept while the contact may be behind our NAT, dropped once its public IPv4 says it is elsewhere.
    let lan = match (contact.local_ip, contact.local_port) {
        (Some(v4), Some(port)) => Some(SocketAddr::new(IpAddr::V4(v4), port)),
        _ => None,
    };
    eps.extend(peers_judged(contact.ip.into_iter().chain(lan).map(|a| (a, false)), crate::network::traverse::our_public_v4()));
    for ep in &contact.device_endpoints {
        eps.extend(endpoint_peers(ep));
    }
    eps
}

/// The addresses at which `contact` might be reachable, without our-LAN context: a peer LAN candidate is kept as long as the address is a usable LAN v4.
///
/// This is what `Contact::race_addrs` and the punch-candidate gathers use. Each device's addresses are still judged against our public IPv4 (`endpoint_peers`), so a LAN address from another network, or our own NAT's outside, never becomes a candidate once that is known.
pub fn gather_peer_candidates(contact: &Contact) -> CandidateSet {
    fgtw::traverse::gather::gather_peer_candidates(
        &contact_endpoints(contact),
        contact.p2p_addr,
        LanPolicy::AnyUsable,
    )
}

/// As [`gather_peer_candidates`], but for the send-decision sites that DO know our own LAN v4:
/// a peer's private-v4 candidate is only kept when it shares our `/24`, so we never punch or retransmit toward a foreign-LAN black hole.
pub fn gather_peer_candidates_from(contact: &Contact, our_v4: Option<Ipv4Addr>) -> CandidateSet {
    fgtw::traverse::gather::gather_peer_candidates(
        &contact_endpoints(contact),
        contact.p2p_addr,
        LanPolicy::SameSubnetAs(our_v4),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::contact::DeviceEndpoint;

    fn a(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }
    fn v4(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    fn contact() -> Contact {
        Contact::from_pin([0u8; 64], [1u8; 32], [2u8; 32], None)
    }

    fn endpoint(public: Option<SocketAddr>, lan: Option<SocketAddr>) -> DeviceEndpoint {
        let mut ep = DeviceEndpoint::new([9u8; 32]);
        ep.online = true;
        for a in [lan, public].into_iter().flatten() {
            ep.note(a, crate::types::contact::AddrSource::Heard);
        }
        ep
    }

    /// Several addresses per device (Nick 2026-11-01: "I could have two ipv6 addy's per device, or four"): every one is a candidate, and a device heard at a LAN address and two public ones keeps all three.
    #[test]
    fn every_address_a_device_is_heard_at_is_a_candidate() {
        let mut ep = DeviceEndpoint::new([9u8; 32]);
        ep.note(a("[2001:db8::1]:4383"), crate::types::contact::AddrSource::Heard);
        ep.note(a("[2001:db8::2]:4383"), crate::types::contact::AddrSource::Record);
        ep.note(a("192.168.1.5:4383"), crate::types::contact::AddrSource::Beacon);
        ep.note(a("[2001:db8::1]:4383"), crate::types::contact::AddrSource::Heard);
        assert_eq!(ep.addrs.len(), 3, "a re-heard address is refreshed, never duplicated");
        assert_eq!(ep.public(), Some(a("[2001:db8::1]:4383")), "the most recently heard public address leads");
        assert_eq!(ep.lan(), Some(a("192.168.1.5:4383")));
        let eps = endpoint_peers(&ep);
        assert_eq!(eps.len(), 3);
        ep.forget(a("[2001:db8::2]:4383"));
        assert_eq!(ep.addrs.len(), 2);
    }

    /// The active device's `ip` + `local_ip`/`local_port` pair must flatten into the same candidates a per-device endpoint would produce — that equivalence is the whole basis for collapsing the two former gathers onto one core.
    #[test]
    fn the_active_device_flattens_like_any_other_endpoint() {
        let mut c = contact();
        c.ip = Some(a("203.0.113.7:4383"));
        c.local_ip = Some(v4("192.168.1.5"));
        c.local_port = Some(4383);

        let set = gather_peer_candidates(&c);
        let addrs: Vec<_> = set.sorted().iter().map(|x| x.addr).collect();
        assert!(addrs.contains(&a("203.0.113.7:4383")));
        assert!(addrs.contains(&a("192.168.1.5:4383")));
        // LAN outranks the punched WAN path.
        assert_eq!(set.best_pair().unwrap().0, a("192.168.1.5:4383"));
    }

    #[test]
    fn a_lan_ip_without_a_port_contributes_nothing() {
        let mut c = contact();
        c.local_ip = Some(v4("192.168.1.5"));
        c.local_port = None;
        assert!(gather_peer_candidates(&c).is_empty());
    }

    /// The reason all endpoints are scanned rather than just the active `ip`: a sibling reachable over v6 must outrank a v4 LAN address that may be on a foreign network.
    #[test]
    fn a_device_endpoints_v6_outranks_the_active_devices_v4_lan() {
        let mut c = contact();
        c.local_ip = Some(v4("192.168.1.5"));
        c.local_port = Some(4383);
        c.device_endpoints = vec![endpoint(Some(a("[2001:db8::1]:4383")), None)];
        assert_eq!(
            gather_peer_candidates(&c).best_pair().unwrap().0,
            a("[2001:db8::1]:4383")
        );
    }

    #[test]
    fn the_subnet_gate_is_the_only_difference_between_the_two_entry_points() {
        let mut c = contact();
        c.ip = Some(a("203.0.113.7:4383"));
        c.local_ip = Some(v4("192.168.9.5")); // a FOREIGN /24
        c.local_port = Some(4383);

        assert_eq!(gather_peer_candidates(&c).sorted().len(), 2);
        assert_eq!(
            gather_peer_candidates_from(&c, Some(v4("192.168.1.9")))
                .sorted()
                .len(),
            1,
            "a foreign LAN address must not survive the subnet gate"
        );
    }

    /// Group membership vouches reachability, so Wi-Fi Direct bypasses the /24 gate that would otherwise reject 192.168.49.x as a foreign LAN.
    #[test]
    fn wifi_direct_survives_the_subnet_gate() {
        let mut c = contact();
        c.p2p_addr = Some(a("192.168.49.5:4383"));
        let set = gather_peer_candidates_from(&c, Some(v4("192.168.1.9")));
        assert_eq!(set.sorted().len(), 1);
        assert_eq!(set.sorted()[0].kind, fgtw::traverse::CandidateKind::HostV4P2p);
    }

    #[test]
    fn the_relay_sentinel_never_survives_flattening() {
        let mut c = contact();
        c.ip = Some(a("0.0.0.0:0"));
        c.p2p_addr = Some(a("0.0.0.0:0"));
        c.device_endpoints = vec![endpoint(Some(a("0.0.0.0:0")), None)];
        assert!(gather_peer_candidates(&c).is_empty());
    }
    fn judged(addrs: &[(&str, bool)], ours: Option<&str>) -> Vec<SocketAddr> {
        let eps = peers_judged(addrs.iter().map(|(s, p)| (a(s), *p)).collect::<Vec<_>>().into_iter(), ours.map(v4));
        eps.iter().flat_map(|p| p.lan.into_iter().chain(p.public)).collect()
    }

    /// A device behind another NAT: its private address names a host on ITS network, however much it looks like ours, so it is dropped unless we heard the device there (2026-11-26: a friend's 192.168.1.10 tried from Nick's 192.168.1.x).
    #[test]
    fn another_networks_lan_address_is_not_ours_to_try() {
        let got = judged(&[("174.45.252.101:4383", false), ("192.168.1.10:4383", false)], Some("97.46.103.153"));
        assert_eq!(got, vec![a("174.45.252.101:4383")], "their LAN address is dropped, their public one kept");
        let heard = judged(&[("174.45.252.101:4383", false), ("192.168.1.10:4383", true)], Some("97.46.103.153"));
        assert!(heard.contains(&a("192.168.1.10:4383")), "an address the device was heard at from here is proven");
    }

    /// A device behind our NAT: its LAN address is the path, and its public address, ours, is our own NAT's outside, which maps back to us (2026-11-26: Esme's device on Nick's network, reached only thru the relay).
    #[test]
    fn a_device_behind_our_nat_goes_by_its_lan_address() {
        let got = judged(&[("97.46.103.153:4383", false), ("192.168.1.42:4383", false)], Some("97.46.103.153"));
        assert_eq!(got, vec![a("192.168.1.42:4383")]);
    }

    /// Our public IPv4 not yet learned: nothing is judged, and the caller's LAN policy alone applies.
    #[test]
    fn nothing_is_judged_before_our_public_address_is_known() {
        let got = judged(&[("174.45.252.101:4383", false), ("192.168.1.10:4383", false)], None);
        assert_eq!(got.len(), 2);
    }
}
