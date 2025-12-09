//! PT Transport Layer
//!
//! Photon Transport over UDP - lean multicast TCP-like reliability
//!
//! Transport priority:
//! 1. UDP (primary - cross-platform, low latency, we handle reliability)
//! 2. TCP (fallback if UDP blocked by network)
//!
//! UDP packets:
//! - DATA: [4-byte seq][payload]
//! - Control: VSF format (detected by VSF header magic)
//!
//! TCP fallback:
//! - Length-prefixed streams: [4-byte len][payload]

use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::time::Duration;

/// Default port for UDP transport
pub const UDP_PORT: u16 = 25401;

/// Default port for TCP fallback
pub const TCP_FALLBACK_PORT: u16 = 25400;

/// Transport mode
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportMode {
    /// UDP (primary - cross-platform, low latency)
    Udp,
    /// TCP fallback (reliable, higher latency)
    Tcp,
}

/// UDP transport (cross-platform, primary)
pub struct UdpTransport {
    socket: UdpSocket,
}

impl UdpTransport {
    /// Create new UDP transport bound to local address
    pub fn new(local_addr: Ipv4Addr) -> io::Result<Self> {
        let addr = SocketAddr::new(IpAddr::V4(local_addr), UDP_PORT);
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        Ok(Self { socket })
    }

    /// Send data to peer
    pub fn send_to(&self, data: &[u8], peer: SocketAddr) -> io::Result<usize> {
        self.socket.send_to(data, peer)
    }

    /// Receive data from any peer
    pub fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.socket.recv_from(buf)
    }

    /// Get local address
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }
}

/// TCP transport for fallback
pub struct TcpTransport {
    listener: Option<TcpListener>,
}

impl TcpTransport {
    /// Create new TCP transport
    pub fn new() -> io::Result<Self> {
        Ok(Self { listener: None })
    }

    /// Start listening on fallback port
    pub fn listen(&mut self, addr: SocketAddr) -> io::Result<()> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        self.listener = Some(listener);
        Ok(())
    }

    /// Accept incoming connection
    pub fn accept(&self) -> io::Result<Option<(TcpStream, SocketAddr)>> {
        if let Some(ref listener) = self.listener {
            match listener.accept() {
                Ok((stream, addr)) => Ok(Some((stream, addr))),
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
                Err(e) => Err(e),
            }
        } else {
            Ok(None)
        }
    }

    /// Connect to peer for sending
    pub fn connect(peer: SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
        let stream = TcpStream::connect_timeout(&peer, timeout)?;
        stream.set_nodelay(true)?; // Disable Nagle's algorithm
        Ok(stream)
    }

    /// Send entire payload over TCP (length-prefixed)
    pub fn send_payload(stream: &mut TcpStream, data: &[u8]) -> io::Result<()> {
        // 4-byte length prefix (big-endian)
        let len_bytes = (data.len() as u32).to_be_bytes();
        stream.write_all(&len_bytes)?;
        stream.write_all(data)?;
        stream.flush()?;
        Ok(())
    }

    /// Receive entire payload from TCP (length-prefixed)
    pub fn recv_payload(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
        // Read 4-byte length prefix
        let mut len_bytes = [0u8; 4];
        stream.read_exact(&mut len_bytes)?;
        let len = u32::from_be_bytes(len_bytes) as usize;

        // Sanity check - max 10MB
        if len > 10 * 1024 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Payload too large",
            ));
        }

        // Read payload
        let mut data = vec![0u8; len];
        stream.read_exact(&mut data)?;
        Ok(data)
    }
}

/// Unified transport that uses UDP or TCP
pub struct PhotonTransport {
    /// Transport mode
    pub mode: TransportMode,
    /// UDP transport (primary)
    udp: Option<UdpTransport>,
    /// TCP transport (fallback)
    tcp: TcpTransport,
    /// Local address
    #[allow(dead_code)]
    local_addr: Ipv4Addr,
}

impl PhotonTransport {
    /// Create new transport
    pub fn new(local_addr: Ipv4Addr) -> io::Result<Self> {
        // Create UDP transport (primary)
        let udp = match UdpTransport::new(local_addr) {
            Ok(u) => Some(u),
            Err(e) => {
                crate::log_error(&format!("Failed to create UDP transport: {}", e));
                None
            }
        };

        let mode = if udp.is_some() {
            TransportMode::Udp
        } else {
            crate::log_info("UDP unavailable, using TCP fallback");
            TransportMode::Tcp
        };

        // Create TCP transport as fallback
        let mut tcp = TcpTransport::new()?;
        let tcp_addr = SocketAddr::new(IpAddr::V4(local_addr), TCP_FALLBACK_PORT);
        if let Err(e) = tcp.listen(tcp_addr) {
            crate::log_error(&format!("Failed to bind TCP fallback: {}", e));
        }

        Ok(Self {
            mode,
            udp,
            tcp,
            local_addr,
        })
    }

    /// Send large payload to peer
    /// For UDP: uses windowed transfer (PT)
    /// For TCP: sends entire payload in one shot
    pub fn send_large(&mut self, peer: Ipv4Addr, data: &[u8]) -> io::Result<()> {
        match self.mode {
            TransportMode::Udp => self.send_udp_pt(peer, data),
            TransportMode::Tcp => self.send_tcp(peer, data),
        }
    }

    /// Send via TCP fallback
    fn send_tcp(&self, peer: Ipv4Addr, data: &[u8]) -> io::Result<()> {
        let addr = SocketAddr::new(IpAddr::V4(peer), TCP_FALLBACK_PORT);
        let mut stream = TcpTransport::connect(addr, Duration::from_secs(10))?;
        TcpTransport::send_payload(&mut stream, data)?;
        crate::log_info(&format!(
            "Sent {} bytes to {} via TCP fallback",
            data.len(),
            peer
        ));
        Ok(())
    }

    /// Send via UDP with PT windowing
    fn send_udp_pt(&self, peer: Ipv4Addr, data: &[u8]) -> io::Result<()> {
        let udp = self
            .udp
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "UDP socket not available"))?;

        // UDP MTU-safe packet size (typical MTU 1500 - IP/UDP headers)
        const PACKET_SIZE: usize = 1400;
        let total_packets = (data.len() + PACKET_SIZE - 1) / PACKET_SIZE;

        let peer_addr = SocketAddr::new(IpAddr::V4(peer), UDP_PORT);

        // Send SPEC first
        let data_hash = *blake3::hash(data).as_bytes();
        let spec = PlptSpec {
            total_packets: total_packets as u32,
            packet_size: PACKET_SIZE as u16,
            total_size: data.len() as u32,
            data_hash,
        };
        udp.send_to(&spec.to_bytes(), peer_addr)?;

        // Simple stop-and-wait for now
        // TODO: Integrate with full PT windowing
        for seq in 0..total_packets {
            let start = seq * PACKET_SIZE;
            let end = ((seq + 1) * PACKET_SIZE).min(data.len());
            let payload = &data[start..end];

            let mut packet = Vec::with_capacity(4 + payload.len());
            packet.extend_from_slice(&(seq as u32).to_be_bytes());
            packet.extend_from_slice(payload);

            udp.send_to(&packet, peer_addr)?;

            // Brief pause to avoid overwhelming receiver
            if seq % 10 == 0 {
                std::thread::sleep(Duration::from_micros(100));
            }
        }

        crate::log_info(&format!(
            "Sent {} bytes ({} packets) to {} via UDP",
            data.len(),
            total_packets,
            peer
        ));

        Ok(())
    }

    /// Poll for incoming data
    /// Returns (data, source) if a complete payload was received
    pub fn poll_recv(&mut self) -> io::Result<Option<(Vec<u8>, Ipv4Addr)>> {
        // Check TCP first (most reliable)
        if let Some((mut stream, addr)) = self.tcp.accept()? {
            if let IpAddr::V4(ipv4) = addr.ip() {
                let data = TcpTransport::recv_payload(&mut stream)?;
                crate::log_info(&format!(
                    "Received {} bytes from {} via TCP",
                    data.len(),
                    ipv4
                ));
                return Ok(Some((data, ipv4)));
            }
        }

        // Check UDP socket
        if let Some(ref udp) = self.udp {
            let mut buf = vec![0u8; 65536];
            match udp.recv_from(&mut buf) {
                Ok((len, src)) if len > 0 => {
                    buf.truncate(len);
                    if let IpAddr::V4(ipv4) = src.ip() {
                        // TODO: Reassemble PT packets
                        return Ok(Some((buf, ipv4)));
                    }
                }
                Ok(_) => {}
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e),
            }
        }

        Ok(None)
    }

    /// Get current transport mode
    pub fn mode(&self) -> TransportMode {
        self.mode
    }
}

/// Simple SPEC packet for PT transfers
struct PlptSpec {
    total_packets: u32,
    packet_size: u16,
    total_size: u32,
    data_hash: [u8; 32],
}

impl PlptSpec {
    fn to_bytes(&self) -> Vec<u8> {
        // Simple binary format for PT SPEC
        // [magic:2][total_packets:4][packet_size:2][total_size:4][hash:32]
        let mut buf = Vec::with_capacity(44);
        buf.extend_from_slice(b"PS"); // Photon Spec magic
        buf.extend_from_slice(&self.total_packets.to_be_bytes());
        buf.extend_from_slice(&self.packet_size.to_be_bytes());
        buf.extend_from_slice(&self.total_size.to_be_bytes());
        buf.extend_from_slice(&self.data_hash);
        buf
    }

    #[allow(dead_code)]
    fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 44 {
            return None;
        }
        if &data[0..2] != b"PS" {
            return None;
        }

        Some(Self {
            total_packets: u32::from_be_bytes(data[2..6].try_into().ok()?),
            packet_size: u16::from_be_bytes(data[6..8].try_into().ok()?),
            total_size: u32::from_be_bytes(data[8..12].try_into().ok()?),
            data_hash: data[12..44].try_into().ok()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pt_spec_roundtrip() {
        let spec = PlptSpec {
            total_packets: 100,
            packet_size: 1024,
            total_size: 102400,
            data_hash: [0xAB; 32],
        };

        let bytes = spec.to_bytes();
        let parsed = PlptSpec::from_bytes(&bytes).expect("Should parse");

        assert_eq!(parsed.total_packets, 100);
        assert_eq!(parsed.packet_size, 1024);
        assert_eq!(parsed.total_size, 102400);
        assert_eq!(parsed.data_hash, [0xAB; 32]);
    }
}
