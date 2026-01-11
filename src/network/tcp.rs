//! TCP Transport Layer
//!
//! Handles reliable TCP connections with centralized logging.
//! Fallback for: CLUTCH key exchange, attachments, large file transfers.
//! Only used when Photon Transport (UDP) fails after retries.
//!
//! VSF files contain an L (file length) field in the header, so TCP
//! just streams raw VSF bytes - no external framing needed.

#[cfg(feature = "verbose-network")]
use super::inspect::vsf_inspect;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};

/// Payload type identifiers for CLUTCH framing (internal to CLUTCH protocol)
pub mod payload_type {
    pub const CLUTCH_FULL_OFFER: u8 = 0x01;
    pub const CLUTCH_KEM_RESPONSE: u8 = 0x02;
    pub const ATTACHMENT: u8 = 0x10;
}

/// Send VSF data over TCP (no external framing - L is in header)
pub fn send(stream: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
    let _addr = stream
        .peer_addr()
        .unwrap_or_else(|_| "unknown".parse().unwrap());

    #[cfg(feature = "verbose-network")]
    {
        let msg = vsf_inspect(data, "TCP", "TX", &_addr.to_string());
        if !msg.is_empty() {
            crate::log(&msg);
        }
    }

    // No length prefix - VSF header contains L (file length)
    stream.write_all(data)?;
    stream.flush()?;

    Ok(())
}

/// Receive VSF data from TCP by parsing L from header
/// Returns the complete VSF file bytes
pub fn recv(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let _addr = stream
        .peer_addr()
        .unwrap_or_else(|_| "unknown".parse().unwrap());

    // Read enough bytes to parse VSF header and extract L (file length)
    // VSF header starts with: RÅ< (4 bytes) + z + y + b + L + ...
    // We need to read incrementally until we have L

    // First, read magic + enough for header fields (conservatively 64 bytes)
    let mut header_buf = vec![0u8; 64];
    stream.read_exact(&mut header_buf)?;

    // Verify VSF magic
    if header_buf.len() < 4 || &header_buf[0..3] != "RÅ".as_bytes() || header_buf[3] != b'<' {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid VSF magic number",
        ));
    }

    // Parse header to extract file length (L)
    let mut ptr = 4; // After "RÅ<"

    // Parse z (version)
    let _ = vsf::decoding::parse(&header_buf, &mut ptr).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("TCP recv: Failed to parse version: {}", e),
        )
    })?;

    // Parse y (backward compat)
    let _ = vsf::decoding::parse(&header_buf, &mut ptr).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("TCP recv: Failed to parse backward compat: {}", e),
        )
    })?;

    // Parse b (header length)
    let _ = vsf::decoding::parse(&header_buf, &mut ptr).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("TCP recv: Failed to parse header length: {}", e),
        )
    })?;

    // Parse L (file length) - this is what we need!
    let file_length = match vsf::decoding::parse(&header_buf, &mut ptr) {
        Ok(vsf::VsfType::L(len, _)) => len,
        Ok(other) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("TCP recv: Expected L for file length, got {:?}", other),
            ))
        }
        Err(e) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("TCP recv: Failed to parse L field: {}", e),
            ))
        }
    };

    // Sanity check - max 64MB
    if file_length > 64 * 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("VSF file too large: {} bytes", file_length),
        ));
    }

    // Now read the remaining bytes (we already have 64)
    let remaining = file_length.saturating_sub(header_buf.len());
    let mut data = header_buf;
    if remaining > 0 {
        data.resize(file_length, 0);
        stream.read_exact(&mut data[64..])?;
    } else {
        // File was smaller than 64 bytes - truncate
        data.truncate(file_length);
    }

    #[cfg(feature = "verbose-network")]
    {
        let msg = vsf_inspect(&data, "TCP", "RX", &_addr.to_string());
        if !msg.is_empty() {
            crate::log(&msg);
        }
    }

    Ok(data)
}

/// Connect to a peer and send data (blocking)
pub fn connect_and_send(addr: SocketAddr, data: &[u8]) -> std::io::Result<()> {
    let mut stream = TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(10))?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
    send(&mut stream, data)?;
    Ok(())
}

/// Async TCP send for PT fallback (fully async, no blocking)
/// No external framing - VSF header contains L (file length)
pub async fn send_tcp(data: &[u8], addr: SocketAddr) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    use tokio::time::timeout;

    let connect_timeout = std::time::Duration::from_secs(10);
    let write_timeout = std::time::Duration::from_secs(30);

    // Connect with timeout
    let mut stream = timeout(connect_timeout, tokio::net::TcpStream::connect(addr))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "TCP connect timeout"))??;

    // No length prefix - VSF header contains L (file length)
    // Write with timeout
    timeout(write_timeout, async {
        stream.write_all(data).await?;
        stream.flush().await
    })
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "TCP write timeout"))?
}

/// Send a framed CLUTCH message
/// Format: [payload_type:1][handle_proof:32][payload]
pub fn send_clutch(
    addr: SocketAddr,
    payload_type: u8,
    handle_proof: &[u8; 32],
    payload: &[u8],
) -> std::io::Result<()> {
    let mut framed = Vec::with_capacity(1 + 32 + payload.len());
    framed.push(payload_type);
    framed.extend_from_slice(handle_proof);
    framed.extend_from_slice(payload);

    connect_and_send(addr, &framed)
}

/// Parse a received CLUTCH frame
/// Returns (payload_type, handle_proof, payload) if valid
pub fn parse_clutch_frame(data: &[u8]) -> Option<(u8, [u8; 32], Vec<u8>)> {
    if data.len() < 33 {
        return None;
    }

    let payload_type = data[0];
    let mut handle_proof = [0u8; 32];
    handle_proof.copy_from_slice(&data[1..33]);
    let payload = data[33..].to_vec();

    Some((payload_type, handle_proof, payload))
}

/// Async TCP listener wrapper for tokio
#[cfg(not(target_os = "android"))]
pub struct TcpListener {
    inner: tokio::net::TcpListener,
}

#[cfg(not(target_os = "android"))]
impl TcpListener {
    /// Bind to address
    pub async fn bind(addr: SocketAddr) -> std::io::Result<Self> {
        let inner = tokio::net::TcpListener::bind(addr).await?;
        crate::log(&format!("TCP: Listening on {}", addr));
        Ok(Self { inner })
    }

    /// Accept incoming connection
    pub async fn accept(&self) -> std::io::Result<(TcpStream, SocketAddr)> {
        let (stream, addr) = self.inner.accept().await?;
        crate::log(&format!("TCP: Connection from {}", addr));

        // Convert to std TcpStream for blocking I/O
        let std_stream = stream.into_std()?;
        Ok((std_stream, addr))
    }
}
