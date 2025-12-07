//! PLTP Transport Layer
//!
//! Primary: Raw IP protocol 254 (fast, minimal overhead)
//! Fallback: TCP (works everywhere, handles reliability)
//!
//! Protocol 254 packets:
//! - DATA: [4-byte seq][payload] - protocol number is the discriminator
//! - Control: VSF format (detected by VSF header magic)
//!
//! TCP fallback:
//! - Just write entire payload in one shot
//! - TCP handles fragmentation and reliability
//! - Length-prefixed: [4-byte len][payload]

use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};
use std::time::Duration;

/// IP protocol number for Photon transfers
pub const PROTOCOL_254: i32 = 254;

/// Default port for TCP fallback
pub const TCP_FALLBACK_PORT: u16 = 25400;

/// Transport mode
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportMode {
    /// Raw IP protocol 254 (preferred)
    Raw254,
    /// TCP fallback (always works)
    TcpFallback,
}

/// Result of transport detection
pub struct TransportCapabilities {
    /// Whether raw sockets are available
    pub raw_available: bool,
    /// Active transport mode
    pub mode: TransportMode,
}

impl TransportCapabilities {
    /// Detect available transport capabilities
    pub fn detect() -> Self {
        let raw_available = Self::try_raw_socket().is_ok();
        let mode = if raw_available {
            TransportMode::Raw254
        } else {
            TransportMode::TcpFallback
        };

        if !raw_available {
            crate::log_info("Raw sockets unavailable, using TCP fallback (slower)");
        }

        Self {
            raw_available,
            mode,
        }
    }

    /// Try to create a raw socket to test availability
    fn try_raw_socket() -> io::Result<()> {
        // Try to create raw socket
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_RAW, PROTOCOL_254) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        unsafe { libc::close(fd) };
        Ok(())
    }
}

/// Raw IP protocol 254 socket
pub struct Raw254Socket {
    fd: RawFd,
    local_addr: Ipv4Addr,
}

impl Raw254Socket {
    /// Create new raw socket bound to local address
    pub fn new(local_addr: Ipv4Addr) -> io::Result<Self> {
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_RAW, PROTOCOL_254) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        // Set receive timeout
        let timeout = libc::timeval {
            tv_sec: 1,
            tv_usec: 0,
        };
        unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_RCVTIMEO,
                &timeout as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::timeval>() as libc::socklen_t,
            );
        }

        // Bind to local address (optional for raw sockets but good practice)
        let addr = libc::sockaddr_in {
            sin_family: libc::AF_INET as u16,
            sin_port: 0,
            sin_addr: libc::in_addr {
                s_addr: u32::from_ne_bytes(local_addr.octets()),
            },
            sin_zero: [0; 8],
        };

        let result = unsafe {
            libc::bind(
                fd,
                &addr as *const _ as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        };

        if result < 0 {
            let err = io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(err);
        }

        Ok(Self { fd, local_addr })
    }

    /// Send data to peer
    pub fn send_to(&self, data: &[u8], peer: Ipv4Addr) -> io::Result<usize> {
        let addr = libc::sockaddr_in {
            sin_family: libc::AF_INET as u16,
            sin_port: 0, // No port for raw IP
            sin_addr: libc::in_addr {
                s_addr: u32::from_ne_bytes(peer.octets()),
            },
            sin_zero: [0; 8],
        };

        let sent = unsafe {
            libc::sendto(
                self.fd,
                data.as_ptr() as *const libc::c_void,
                data.len(),
                0,
                &addr as *const _ as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        };

        if sent < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(sent as usize)
        }
    }

    /// Receive data from any peer
    /// Returns (data, source_addr)
    pub fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, Ipv4Addr)> {
        let mut addr: libc::sockaddr_in = unsafe { std::mem::zeroed() };
        let mut addr_len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;

        let received = unsafe {
            libc::recvfrom(
                self.fd,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                0,
                &mut addr as *mut _ as *mut libc::sockaddr,
                &mut addr_len,
            )
        };

        if received < 0 {
            return Err(io::Error::last_os_error());
        }

        // Parse source address
        let src_bytes = addr.sin_addr.s_addr.to_ne_bytes();
        let src_addr = Ipv4Addr::new(src_bytes[0], src_bytes[1], src_bytes[2], src_bytes[3]);

        // Raw sockets include IP header (20 bytes typically)
        // Skip IP header to get our protocol data
        let ip_header_len = if received >= 1 {
            ((buf[0] & 0x0F) as usize) * 4
        } else {
            20
        };

        if (received as usize) <= ip_header_len {
            return Ok((0, src_addr));
        }

        // Move payload to start of buffer
        let payload_len = received as usize - ip_header_len;
        buf.copy_within(ip_header_len..received as usize, 0);

        Ok((payload_len, src_addr))
    }

    /// Set non-blocking mode
    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        let flags = unsafe { libc::fcntl(self.fd, libc::F_GETFL) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }

        let new_flags = if nonblocking {
            flags | libc::O_NONBLOCK
        } else {
            flags & !libc::O_NONBLOCK
        };

        if unsafe { libc::fcntl(self.fd, libc::F_SETFL, new_flags) } < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }
}

impl Drop for Raw254Socket {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

impl AsRawFd for Raw254Socket {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
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

/// Unified transport that uses raw254 or TCP as needed
pub struct PhotonTransport {
    /// Transport mode
    pub mode: TransportMode,
    /// Raw socket (if available)
    raw: Option<Raw254Socket>,
    /// TCP transport (always available)
    tcp: TcpTransport,
    /// Local address
    local_addr: Ipv4Addr,
}

impl PhotonTransport {
    /// Create new transport, detecting best available mode
    pub fn new(local_addr: Ipv4Addr) -> io::Result<Self> {
        let caps = TransportCapabilities::detect();

        let raw = if caps.raw_available {
            match Raw254Socket::new(local_addr) {
                Ok(sock) => {
                    sock.set_nonblocking(true)?;
                    Some(sock)
                }
                Err(e) => {
                    crate::log_error(&format!("Failed to create raw socket: {}", e));
                    None
                }
            }
        } else {
            None
        };

        let mode = if raw.is_some() {
            TransportMode::Raw254
        } else {
            TransportMode::TcpFallback
        };

        let mut tcp = TcpTransport::new()?;
        // Always listen on TCP fallback port
        let tcp_addr = SocketAddr::new(IpAddr::V4(local_addr), TCP_FALLBACK_PORT);
        if let Err(e) = tcp.listen(tcp_addr) {
            crate::log_error(&format!("Failed to bind TCP fallback: {}", e));
        }

        Ok(Self {
            mode,
            raw,
            tcp,
            local_addr,
        })
    }

    /// Send large payload to peer
    /// For raw254: uses windowed transfer (PLTP)
    /// For TCP: sends entire payload in one shot
    pub fn send_large(&mut self, peer: Ipv4Addr, data: &[u8]) -> io::Result<()> {
        match self.mode {
            TransportMode::Raw254 => {
                // Use PLTP windowed transfer over raw254
                self.send_raw254_pltp(peer, data)
            }
            TransportMode::TcpFallback => {
                // Just send everything over TCP
                self.send_tcp(peer, data)
            }
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

    /// Send via raw254 with PLTP windowing
    fn send_raw254_pltp(&self, peer: Ipv4Addr, data: &[u8]) -> io::Result<()> {
        let raw = self
            .raw
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Raw socket not available"))?;

        // For raw254, we need to implement PLTP windowing
        // DATA packet format: [4-byte seq][payload]
        const PACKET_SIZE: usize = 1000;
        let total_packets = (data.len() + PACKET_SIZE - 1) / PACKET_SIZE;

        // Send SPEC first (VSF format)
        let data_hash = *blake3::hash(data).as_bytes();
        let spec = Raw254Spec {
            total_packets: total_packets as u32,
            packet_size: PACKET_SIZE as u16,
            total_size: data.len() as u32,
            data_hash,
        };
        raw.send_to(&spec.to_bytes(), peer)?;

        // Simple stop-and-wait for now (can upgrade to full windowing later)
        // TODO: Integrate with full PLTP windowing
        for seq in 0..total_packets {
            let start = seq * PACKET_SIZE;
            let end = ((seq + 1) * PACKET_SIZE).min(data.len());
            let payload = &data[start..end];

            let mut packet = Vec::with_capacity(4 + payload.len());
            packet.extend_from_slice(&(seq as u32).to_be_bytes());
            packet.extend_from_slice(payload);

            raw.send_to(&packet, peer)?;

            // Brief pause to avoid overwhelming receiver
            if seq % 10 == 0 {
                std::thread::sleep(Duration::from_micros(100));
            }
        }

        crate::log_info(&format!(
            "Sent {} bytes ({} packets) to {} via raw254",
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

        // Check raw socket
        if let Some(ref raw) = self.raw {
            let mut buf = vec![0u8; 65536];
            match raw.recv_from(&mut buf) {
                Ok((len, src)) if len > 0 => {
                    buf.truncate(len);
                    // TODO: Reassemble PLTP packets
                    // For now, return raw data
                    return Ok(Some((buf, src)));
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

    /// Check if raw sockets are available
    pub fn has_raw(&self) -> bool {
        self.raw.is_some()
    }
}

/// Simple SPEC packet for raw254 (VSF-encoded)
struct Raw254Spec {
    total_packets: u32,
    packet_size: u16,
    total_size: u32,
    data_hash: [u8; 32],
}

impl Raw254Spec {
    fn to_bytes(&self) -> Vec<u8> {
        // Simple binary format for raw254 SPEC
        // [magic:2][total_packets:4][packet_size:2][total_size:4][hash:32]
        let mut buf = Vec::with_capacity(44);
        buf.extend_from_slice(b"PS"); // Photon Spec magic
        buf.extend_from_slice(&self.total_packets.to_be_bytes());
        buf.extend_from_slice(&self.packet_size.to_be_bytes());
        buf.extend_from_slice(&self.total_size.to_be_bytes());
        buf.extend_from_slice(&self.data_hash);
        buf
    }

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
    fn test_transport_capabilities() {
        let caps = TransportCapabilities::detect();
        // This test just verifies detection doesn't crash
        // Raw sockets likely won't be available in test environment
        println!("Raw available: {}", caps.raw_available);
        println!("Mode: {:?}", caps.mode);
    }

    #[test]
    fn test_raw254_spec_roundtrip() {
        let spec = Raw254Spec {
            total_packets: 100,
            packet_size: 1000,
            total_size: 99500,
            data_hash: [0xAB; 32],
        };

        let bytes = spec.to_bytes();
        let parsed = Raw254Spec::from_bytes(&bytes).expect("Should parse");

        assert_eq!(parsed.total_packets, 100);
        assert_eq!(parsed.packet_size, 1000);
        assert_eq!(parsed.total_size, 99500);
        assert_eq!(parsed.data_hash, [0xAB; 32]);
    }
}
