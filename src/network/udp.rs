//! UDP Transport Layer
//!
//! Handles all UDP network traffic with centralized logging. Used for: ping/pong, status updates, LAN discovery, small messages, streaming. Fallback when Photon Transport is unavailable.

#[cfg(feature = "development")]
use super::inspect::vsf_inspect;
use std::net::SocketAddr;

/// Centralized UDP TX - logs via vsf_inspect then sends This is THE ONLY place UDP packets should be transmitted (except LAN broadcast) Normalize a UNICAST destination for the main dual-stack (`[::]`) photon socket: a v6 socket cannot send to a plain `SocketAddr::V4` — the datagram is silently dropped — it must target the IPv4-mapped form `[::ffff:a.b.c.d]`. Some send paths construct raw V4 dests (e.g. `race_addrs` builds the LAN address from a stored `Ipv4Addr`) while others reuse a kernel-supplied `::ffff:` address (an incoming packet's src), so unicast delivery was inconsistent: ACKs (src-derived, mapped) arrived but chat messages (race_addrs, raw V4) vanished. Mapping here makes every unicast send go out in a form the dual-stack socket accepts. Only the async `send` (always the dual-stack socket) maps; `send_sync` is left raw because it serves v4 multicast/broadcast on dedicated v4 sockets, which must NOT be mapped.
fn map_v4_for_dualstack(addr: SocketAddr) -> SocketAddr {
    match addr {
        SocketAddr::V4(v4) => {
            SocketAddr::new(std::net::IpAddr::V6(v4.ip().to_ipv6_mapped()), v4.port())
        }
        SocketAddr::V6(_) => addr,
    }
}

/// Canonicalise a socket address to plain IPv4 when it's an IPv4-mapped IPv6 (`::ffff:a.b.c.d`). The dual-stack `[::]` socket delivers every inbound v4 datagram in this mapped form, so a raw `src_addr` from `recv_from` and a `race_addrs`-built v4 address compare unequal despite being the same host. Canonicalise before storing, comparing, or wire-encoding an observed address (e.g. the pong reflexive echo and the punch candidate matrix) so the two representations don't flap. Inverse of [`map_v4_for_dualstack`], which is applied only at send time.
pub fn canon_socketaddr(addr: SocketAddr) -> SocketAddr {
    match addr {
        SocketAddr::V6(v6) => match v6.ip().to_ipv4_mapped() {
            Some(v4) => SocketAddr::new(std::net::IpAddr::V4(v4), v6.port()),
            None => addr,
        },
        SocketAddr::V4(_) => addr,
    }
}

pub async fn send(socket: &tokio::net::UdpSocket, data: &[u8], addr: SocketAddr) {
    // An empty payload is never a real datagram — it's PT's "queued, nothing to send now" signal (a small packet waiting behind an in-flight one in the stop-and-wait queue). Skip it so callers don't have to guard every send site.
    if data.is_empty() {
        return;
    }
    let addr = map_v4_for_dualstack(addr);
    #[cfg(feature = "development")]
    {
        let msg = vsf_inspect(data, "UDP", "TX", &addr.to_string());
        if !msg.is_empty() {
            crate::log(&msg);
        }
    }
    let _ = socket.send_to(data, addr).await;
}

/// Synchronous version for non-async contexts (LAN broadcast uses std::net::UdpSocket)
pub fn send_sync(
    socket: &std::net::UdpSocket,
    data: &[u8],
    addr: SocketAddr,
) -> std::io::Result<usize> {
    #[cfg(feature = "development")]
    {
        let msg = vsf_inspect(data, "UDP", "TX", &addr.to_string());
        if !msg.is_empty() {
            crate::log(&msg);
        }
    }
    socket.send_to(data, addr)
}

/// Log received UDP packet (call this in the receive loop)
#[cfg(feature = "development")]
pub fn log_received(data: &[u8], addr: &SocketAddr) {
    let msg = vsf_inspect(data, "UDP", "RX", &addr.to_string());
    if !msg.is_empty() {
        crate::log(&msg);
    }
}

/// Get local LAN IP address by connecting to external address This finds which interface the OS would use to reach the internet
pub fn get_local_ip() -> Option<std::net::Ipv4Addr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    // Connect to Cloudflare DNS - doesn't actually send packets, just sets up routing
    socket.connect("1.1.1.1:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if is_usable_lan_ipv4(ip) => Some(ip),
        _ => None,
    }
}

/// Is `ip` an address another host on our LAN could actually reach us at?
/// Rejects `192.0.0.0/24` — the IETF Protocol Assignments block (RFC 6890), whose `192.0.0.0/29` service-continuity prefix (RFC 7335) is what Android's 464XLAT CLAT hands a cellular device (`192.0.0.4`). That address is meaningful ONLY on the device's own stack; published as a peer's `local_ip` it is pure noise that makes every other device burn its PT/TCP retry budget (~17s observed) racing an unreachable candidate before the WAN path wins. Also rejects loopback and link-local, which are never useful peer LAN addresses. A cellular device thus advertises NO LAN address (correct — it has none), and its reachable WAN IPv6 carries the traffic.
pub use fgtw::traverse::gather::is_usable_lan_ipv4;

/// True for the RFC 1918 private ranges (10/8, 172.16/12, 192.168/16) — the addresses that are only reachable on a shared LAN. Used to decide whether a peer's v4 candidate is a routable public address (send freely) or a private one that's only worth trying when we're on the SAME subnet (see gather::is_foreign_peer_lan).
pub use fgtw::traverse::gather::is_private_ipv4;

/// Get LAN broadcast address for the interface that routes to internet Returns (broadcast_addr, local_ip) or None if unable to determine
///
/// On Linux: parses `ip addr` output to find actual broadcast address Fallback: assumes /24 subnet and computes broadcast from local IP
pub fn get_broadcast_addr() -> Option<(std::net::Ipv4Addr, std::net::Ipv4Addr)> {
    let local_ip = get_local_ip()?;

    // Try to get actual broadcast address from system
    #[cfg(target_os = "linux")]
    {
        if let Some(broadcast) = get_broadcast_from_system(&local_ip) {
            return Some((broadcast, local_ip));
        }
    }

    // Fallback: assume /24 subnet (most common home/office network) e.g., a.b.c.d -> a.b.c.255
    let octets = local_ip.octets();
    let broadcast = std::net::Ipv4Addr::new(octets[0], octets[1], octets[2], 255);
    Some((broadcast, local_ip))
}

/// Parse broadcast address from `ip addr` output on Linux
#[cfg(target_os = "linux")]
fn get_broadcast_from_system(local_ip: &std::net::Ipv4Addr) -> Option<std::net::Ipv4Addr> {
    use std::process::Command;

    let output = Command::new("ip").args(["addr", "show"]).output().ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let local_str = local_ip.to_string();

    // Find line containing our local IP and extract broadcast address Format: "inet a.b.c.d/24 brd a.b.c.255 scope global ..."
    for line in stdout.lines() {
        if line.contains(&local_str) && line.contains("brd") {
            // Parse: inet IP/prefix brd BROADCAST ...
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(brd_idx) = parts.iter().position(|&s| s == "brd") {
                if let Some(brd_addr) = parts.get(brd_idx + 1) {
                    if let Ok(broadcast) = brd_addr.parse::<std::net::Ipv4Addr>() {
                        return Some(broadcast);
                    }
                }
            }
        }
    }
    None
}

/// Parse LAN discovery packet Returns (handle_proof, ip, port, device_pubkey) if valid, None otherwise handle_proof is extracted from the VSF header's provenance hash (hp) device_pubkey (ke) is None on beacons from builds that predate it; receivers use it to drop their OWN looped-back beacon (a fleet shares one handle_proof, so the handle alone can't tell self from sibling).
pub fn parse_lan_discovery(
    packet: &[u8],
    src_addr: SocketAddr,
) -> Option<([u8; 32], std::net::Ipv4Addr, u16, Option<[u8; 32]>)> {
    use vsf::file_format::VsfHeader;
    use vsf::VsfType;

    // Parse header to get provenance hash (sender identity) and find section start Note: No is_original() check - LAN discovery is a simple unsigned broadcast
    let (header, header_end) = VsfHeader::decode(packet).ok()?;

    // Extract handle_proof from header provenance hash (by ref — the header is still needed below to resolve the TOC-named section)
    let handle_proof = match &header.provenance_hash {
        VsfType::hp(bytes) if bytes.len() == 32 => {
            let mut hp = [0u8; 32];
            hp.copy_from_slice(bytes);
            hp
        }
        _ => return None,
    };

    // Resolve the section via `primary_section`, NOT a bare `VsfSection::parse`: the section NAME lives in the header TOC (near-form), so a body parse returns `section.name == ""` and the `== "pt_disc"` check below silently rejected EVERY beacon on EVERY device — LAN discovery dead fleet-wide, no log line anywhere, same-LAN peers stuck learning each other from (WAN-only-when-multi-homed) registry records and parking on relay (field, 2026-08-11). Third documented victim of this trap after the relay pipe and the hub push accelerator (see relay.rs).
    let section = header.primary_section(packet, header_end).ok()?;

    if section.name != "pt_disc" {
        return None;
    }

    // Extract port — width-agnostic: a writer may auto-size, and a port under 256 legitimately encodes as u3.
    let port = match section.get_field("port") {
        Some(field) => match field
            .values
            .first()
            .and_then(|v| v.as_u64())
            .and_then(|n| u16::try_from(n).ok())
        {
            Some(p) => p,
            None => return None,
        },
        None => return None,
    };

    // Sender's device pubkey (ke) — optional for wire compat with pre-ke beacons
    let device_pubkey = section
        .get_field("ke")
        .and_then(|f| match f.values.first() {
            Some(VsfType::ke(bytes)) if bytes.len() == 32 => {
                let mut k = [0u8; 32];
                k.copy_from_slice(bytes);
                Some(k)
            }
            _ => None,
        });

    // Handle both native IPv4 and IPv4-mapped IPv6 addresses
    let src_ip = match src_addr.ip() {
        std::net::IpAddr::V4(ip) => ip,
        std::net::IpAddr::V6(ip6) => ip6.to_ipv4_mapped()?,
    };

    Some((handle_proof, src_ip, port, device_pubkey))
}

/// Build LAN discovery broadcast packet handle_proof is stored in VSF header as provenance hash (hp) for identity One-shot broadcast - no rolling hash needed (provenance_only) device_pubkey rides along (ke) so receivers can tell WHICH fleet device is beaconing — and in particular drop their own looped-back beacon instead of learning themselves as a peer.
pub fn build_lan_discovery(handle_proof: [u8; 32], port: u16, device_pubkey: [u8; 32]) -> Vec<u8> {
    use vsf::{VsfBuilder, VsfType};

    VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_hash(handle_proof) // Identity in header - no registry lookup needed
        .provenance_only() // No rolling hash - one-shot broadcast
        .add_section(
            "pt_disc",
            vec![
                ("port".to_string(), VsfType::u(port as usize, false)),
                ("ke".to_string(), VsfType::ke(device_pubkey.to_vec())),
            ],
        )
        .build()
        .unwrap_or_default()
}

#[cfg(test)]
mod lan_addr_tests {
    use super::is_usable_lan_ipv4;
    use std::net::Ipv4Addr;

    /// The discovery beacon must round-trip thru its own parser. Field, 2026-08-11: every device logged Multicast RX for every beacon and ZERO "Discovered peer" — the parse failed fleet-wide, silently, which is why LAN candidates only ever came from the (WAN-only-when-multi-homed) registry records and same-LAN peers parked on relay.
    #[test]
    fn lan_discovery_beacon_round_trips() {
        let packet = super::build_lan_discovery([7u8; 32], 4383, [9u8; 32]);
        assert!(!packet.is_empty(), "beacon build produced no bytes");
        let src: std::net::SocketAddr = "192.168.1.5:9999".parse().unwrap();
        let parsed = super::parse_lan_discovery(&packet, src);
        let (hp, ip, port, dev) = parsed.expect("our own beacon must parse");
        assert_eq!(hp, [7u8; 32]);
        assert_eq!(
            ip,
            Ipv4Addr::new(192, 168, 1, 5),
            "local ip comes from the observed source"
        );
        assert_eq!(port, 4383);
        assert_eq!(dev, Some([9u8; 32]));
    }

    #[test]
    fn rejects_clat_and_specials_keeps_real_lan() {
        // 464XLAT CLAT + the whole service-continuity /24 → unusable.
        assert!(!is_usable_lan_ipv4(Ipv4Addr::new(192, 0, 0, 4)));
        assert!(!is_usable_lan_ipv4(Ipv4Addr::new(192, 0, 0, 1)));
        assert!(!is_usable_lan_ipv4(Ipv4Addr::new(127, 0, 0, 1)));
        assert!(!is_usable_lan_ipv4(Ipv4Addr::new(169, 254, 1, 1)));
        assert!(!is_usable_lan_ipv4(Ipv4Addr::new(0, 0, 0, 0)));
        // Real private LANs → usable. Note 192.0.1.x and 192.168.x are NOT in 192.0.0.0/24.
        assert!(is_usable_lan_ipv4(Ipv4Addr::new(192, 168, 0, 197)));
        assert!(is_usable_lan_ipv4(Ipv4Addr::new(10, 0, 0, 5)));
        assert!(is_usable_lan_ipv4(Ipv4Addr::new(192, 0, 1, 4)));
    }
}
