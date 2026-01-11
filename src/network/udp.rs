//! UDP Transport Layer
//!
//! Handles all UDP network traffic with centralized logging.
//! Used for: ping/pong, status updates, LAN discovery, small messages, streaming.
//! Fallback when Photon Transport is unavailable.

#[cfg(feature = "verbose-network")]
use super::inspect::vsf_inspect;
use std::net::SocketAddr;

/// Centralized UDP TX - logs via vsf_inspect then sends
/// This is THE ONLY place UDP packets should be transmitted (except LAN broadcast)
pub(crate) async fn send(socket: &tokio::net::UdpSocket, data: &[u8], addr: SocketAddr) {
    #[cfg(feature = "verbose-network")]
    {
        let msg = vsf_inspect(data, "UDP", "TX", &addr.to_string());
        if !msg.is_empty() {
            crate::log(&msg);
        }
    }
    let _ = socket.send_to(data, addr).await;
}

/// Synchronous version for non-async contexts (LAN broadcast uses std::net::UdpSocket)
pub(crate) fn send_sync(
    socket: &std::net::UdpSocket,
    data: &[u8],
    addr: SocketAddr,
) -> std::io::Result<usize> {
    #[cfg(feature = "verbose-network")]
    {
        let msg = vsf_inspect(data, "UDP", "TX", &addr.to_string());
        if !msg.is_empty() {
            crate::log(&msg);
        }
    }
    socket.send_to(data, addr)
}

/// Log received UDP packet (call this in the receive loop)
#[cfg(feature = "verbose-network")]
pub(crate) fn log_received(data: &[u8], addr: &SocketAddr) {
    let msg = vsf_inspect(data, "UDP", "RX", &addr.to_string());
    if !msg.is_empty() {
        crate::log(&msg);
    }
}

/// Get local LAN IP address by connecting to external address
/// This finds which interface the OS would use to reach the internet
pub(crate) fn get_local_ip() -> Option<std::net::Ipv4Addr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    // Connect to Cloudflare DNS - doesn't actually send packets, just sets up routing
    socket.connect("1.1.1.1:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) => Some(ip),
        _ => None,
    }
}

/// Get LAN broadcast address for the interface that routes to internet
/// Returns (broadcast_addr, local_ip) or None if unable to determine
///
/// On Linux: parses `ip addr` output to find actual broadcast address
/// Fallback: assumes /24 subnet and computes broadcast from local IP
pub(crate) fn get_broadcast_addr() -> Option<(std::net::Ipv4Addr, std::net::Ipv4Addr)> {
    let local_ip = get_local_ip()?;

    // Try to get actual broadcast address from system
    #[cfg(target_os = "linux")]
    {
        if let Some(broadcast) = get_broadcast_from_system(&local_ip) {
            return Some((broadcast, local_ip));
        }
    }

    // Fallback: assume /24 subnet (most common home/office network)
    // e.g., 192.168.1.42 -> 192.168.1.255
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

    // Find line containing our local IP and extract broadcast address
    // Format: "inet 192.168.0.197/24 brd 192.168.0.255 scope global ..."
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

/// Parse LAN discovery packet
/// Returns (handle_proof, ip, port) if valid, None otherwise
/// handle_proof is extracted from the VSF header's provenance hash (hp)
pub(crate) fn parse_lan_discovery(
    packet: &[u8],
    src_addr: SocketAddr,
) -> Option<([u8; 32], std::net::Ipv4Addr, u16)> {
    use vsf::file_format::{VsfHeader, VsfSection};
    use vsf::VsfType;

    // Parse header to get provenance hash (sender identity) and find section start
    // Note: No is_original() check - LAN discovery is a simple unsigned broadcast
    let (header, header_end) = VsfHeader::decode(packet).ok()?;

    // Extract handle_proof from header provenance hash
    let handle_proof = match header.provenance_hash {
        VsfType::hp(bytes) if bytes.len() == 32 => {
            let mut hp = [0u8; 32];
            hp.copy_from_slice(&bytes);
            hp
        }
        _ => return None,
    };

    // Parse section
    let mut ptr = header_end;
    let section = VsfSection::parse(packet, &mut ptr).ok()?;

    if section.name != "pt_disc" {
        return None;
    }

    // Extract port (u4 = u16)
    let port = match section.get_field("port") {
        Some(field) => match field.values.first() {
            Some(VsfType::u4(p)) => *p,
            _ => return None,
        },
        None => return None,
    };

    // Handle both native IPv4 and IPv4-mapped IPv6 addresses
    let src_ip = match src_addr.ip() {
        std::net::IpAddr::V4(ip) => ip,
        std::net::IpAddr::V6(ip6) => ip6.to_ipv4_mapped()?,
    };

    Some((handle_proof, src_ip, port))
}

/// Build LAN discovery broadcast packet
/// handle_proof is stored in VSF header as provenance hash (hp) for identity
/// One-shot broadcast - no rolling hash needed (provenance_only)
pub(crate) fn build_lan_discovery(handle_proof: [u8; 32], port: u16) -> Vec<u8> {
    use vsf::{VsfBuilder, VsfType};

    VsfBuilder::new()
        .creation_time_nanos(vsf::eagle_time_nanos())
        .provenance_hash(handle_proof) // Identity in header - no registry lookup needed
        .provenance_only() // No rolling hash - one-shot broadcast
        .add_section("pt_disc", vec![("port".to_string(), VsfType::u4(port))])
        .build()
        .unwrap_or_default()
}
