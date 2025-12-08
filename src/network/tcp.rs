//! TCP Transport Layer
//!
//! Handles reliable TCP connections with centralized logging.
//! Used for: CLUTCH key exchange, attachments, large file transfers.
//! Fallback when Photon Transport is unavailable.

#[cfg(feature = "development")]
use super::inspect::vsf_inspect;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};

/// Payload type identifiers for TCP framing
pub mod payload_type {
    pub const CLUTCH_FULL_OFFER: u8 = 0x01;
    pub const CLUTCH_KEM_RESPONSE: u8 = 0x02;
    pub const ATTACHMENT: u8 = 0x10;
}

/// Send data over TCP with length prefix and logging
/// Format: [length:4 bytes BE][payload]
pub fn send(stream: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
    let addr = stream
        .peer_addr()
        .unwrap_or_else(|_| "unknown".parse().unwrap());

    #[cfg(feature = "development")]
    {
        let msg = vsf_inspect(data, "TCP", "TX", &addr.to_string());
        if !msg.is_empty() {
            crate::log_info(&msg);
        }
    }

    // Write length prefix (4 bytes, big endian)
    let len = data.len() as u32;
    stream.write_all(&len.to_be_bytes())?;

    // Write payload
    stream.write_all(data)?;
    stream.flush()?;

    Ok(())
}

/// Receive data from TCP with length prefix and logging
/// Returns the payload bytes
pub fn recv(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let addr = stream
        .peer_addr()
        .unwrap_or_else(|_| "unknown".parse().unwrap());

    // Read length prefix
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;

    // Sanity check - max 64MB
    if len > 64 * 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("TCP payload too large: {} bytes", len),
        ));
    }

    // Read payload
    let mut data = vec![0u8; len];
    stream.read_exact(&mut data)?;

    #[cfg(feature = "development")]
    {
        let msg = vsf_inspect(&data, "TCP", "RX", &addr.to_string());
        if !msg.is_empty() {
            crate::log_info(&msg);
        }
    }

    Ok(data)
}

/// Connect to a peer and send data
pub fn connect_and_send(addr: SocketAddr, data: &[u8]) -> std::io::Result<()> {
    let mut stream = TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(10))?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
    send(&mut stream, data)?;
    Ok(())
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
        crate::log_info(&format!("TCP: Listening on {}", addr));
        Ok(Self { inner })
    }

    /// Accept incoming connection
    pub async fn accept(&self) -> std::io::Result<(TcpStream, SocketAddr)> {
        let (stream, addr) = self.inner.accept().await?;
        crate::log_info(&format!("TCP: Connection from {}", addr));

        // Convert to std TcpStream for blocking I/O
        let std_stream = stream.into_std()?;
        Ok((std_stream, addr))
    }
}
