//! UDP Transport Layer
//!
//! Handles all UDP network traffic with centralized logging.
//! Used for: ping/pong, status updates, LAN discovery, small messages, streaming.
//! Fallback when Photon Transport is unavailable.

#[cfg(feature = "development")]
use super::inspect::vsf_inspect;
use std::net::SocketAddr;

/// Centralized UDP TX - logs via vsf_inspect then sends
/// This is THE ONLY place UDP packets should be transmitted (except LAN broadcast)
pub async fn send(socket: &tokio::net::UdpSocket, data: &[u8], addr: SocketAddr) {
    #[cfg(feature = "development")]
    {
        let msg = vsf_inspect(data, "UDP", "TX", &addr.to_string());
        if !msg.is_empty() {
            crate::log_info(&msg);
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
            crate::log_info(&msg);
        }
    }
    socket.send_to(data, addr)
}

/// Log received UDP packet (call this in the receive loop)
#[cfg(feature = "development")]
pub fn log_received(data: &[u8], addr: &SocketAddr) {
    let msg = vsf_inspect(data, "UDP", "RX", &addr.to_string());
    if !msg.is_empty() {
        crate::log_info(&msg);
    }
}

/// Get local LAN IP address by connecting to external address
/// This finds which interface the OS would use to reach the internet
pub fn get_local_ip() -> Option<std::net::Ipv4Addr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    // Connect to Google DNS - doesn't actually send packets, just sets up routing
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) => Some(ip),
        _ => None,
    }
}

/// Parse LAN discovery packet
/// Returns (handle_proof, ip, port) if valid, None otherwise
/// handle_proof is extracted from the VSF header's provenance hash (hp)
pub fn parse_lan_discovery(
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
pub fn build_lan_discovery(handle_proof: [u8; 32], port: u16) -> Vec<u8> {
    use vsf::{VsfBuilder, VsfType};

    VsfBuilder::new()
        .creation_time_nanos(vsf::eagle_time_nanos())
        .provenance_hash(handle_proof) // Identity in header - no registry lookup needed
        .provenance_only() // No rolling hash - one-shot broadcast
        .add_section(
            "pt_disc",
            vec![("port".to_string(), VsfType::u4(port))],
        )
        .build()
        .unwrap_or_default()
}
