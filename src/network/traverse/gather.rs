//! Candidate gathering — photon's `Contact` adapter over [`fgtw::traverse::gather`].
//!
//! The gathering logic itself lives in the shared crate so photon and rustdesk can't drift.
//! What stays here is the part that is genuinely photon's: flattening a [`Contact`] — with its
//! active-device address, its per-device endpoint list, and its Wi-Fi Direct group address —
//! into the crate's [`PeerEndpoint`] shape.
//!
//! The two entry points differ only in whether a peer's LAN address must share our `/24`;
//! that is now [`LanPolicy`] rather than two near-identical copies of the same loop.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use crate::types::contact::Contact;
use fgtw::traverse::candidate::CandidateSet;
use fgtw::traverse::gather::{LanPolicy, PeerEndpoint};

// Re-exported so the ~23 existing `traverse::gather::…` call sites across photon keep
// resolving unchanged.
pub use fgtw::traverse::gather::{
    gather_own_candidates, is_bogus_addr, is_foreign_peer_lan, is_private_ipv4, is_usable_lan_ipv4,
    is_wfd_subnet, peer_lan_reachable, public_kind,
};

/// Flatten a contact into the crate's endpoint shape: the active device first (its `ip` plus
/// its `local_ip`/`local_port` pair), then every learned per-device endpoint.
///
/// Scanning all of them, not just the active `ip`, is what surfaces a peer's global IPv6 when
/// the active address happens to be v4 — so the v6 host, priority-first, gets tried before a
/// v4 LAN address that may be on a foreign network.
fn contact_endpoints(contact: &Contact) -> Vec<PeerEndpoint> {
    let mut eps = Vec::with_capacity(contact.device_endpoints.len() + 1);
    eps.push(PeerEndpoint {
        public: contact.ip,
        lan: match (contact.local_ip, contact.local_port) {
            (Some(v4), Some(port)) => Some(SocketAddr::new(IpAddr::V4(v4), port)),
            _ => None,
        },
    });
    for ep in &contact.device_endpoints {
        eps.push(PeerEndpoint {
            public: ep.public,
            lan: ep.lan,
        });
    }
    eps
}

/// The addresses at which `contact` might be reachable, without our-LAN context: a peer LAN
/// candidate is kept as long as the address is a usable LAN v4.
///
/// This is what `Contact::race_addrs` and the punch-candidate gathers use — threading our-LAN
/// through every send site would be a wide and risky change, and a foreign address here
/// merely fails to validate rather than causing harm.
pub fn gather_peer_candidates(contact: &Contact) -> CandidateSet {
    fgtw::traverse::gather::gather_peer_candidates(
        &contact_endpoints(contact),
        contact.p2p_addr,
        LanPolicy::AnyUsable,
    )
}

/// As [`gather_peer_candidates`], but for the send-decision sites that DO know our own LAN v4:
/// a peer's private-v4 candidate is only kept when it shares our `/24`, so we never punch or
/// retransmit toward a foreign-LAN black hole.
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
        DeviceEndpoint {
            pubkey: [9u8; 32],
            public,
            lan,
            online: true,
        }
    }

    /// The active device's `ip` + `local_ip`/`local_port` pair must flatten into the same
    /// candidates a per-device endpoint would produce — that equivalence is the whole basis
    /// for collapsing the two former gathers onto one core.
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

    /// The reason all endpoints are scanned rather than just the active `ip`: a sibling
    /// reachable over v6 must outrank a v4 LAN address that may be on a foreign network.
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

    /// Group membership vouches reachability, so Wi-Fi Direct bypasses the /24 gate that
    /// would otherwise reject 192.168.49.x as a foreign LAN.
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
}
