//! TCP Transport Layer
//!
//! Handles reliable TCP connections with centralized logging. Fallback for: CLUTCH key exchange, attachments, large file transfers. Only used when Photon Transport (UDP) fails after retries.
//!
//! VSF files contain an L (file length) field in the header, so TCP just streams raw VSF bytes - no external framing needed.

#[cfg(feature = "development")]
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
    #[cfg(feature = "development")]
    {
        // A peer address the OS cannot name is labelled, never a panic (the old fallback parsed "unknown" as a SocketAddr, which always fails).
        let addr = stream.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "unknown".to_string());
        let msg = vsf_inspect(data, "TCP", "TX", &addr);
        if !msg.is_empty() {
            crate::log(&msg);
        }
    }

    // No length prefix - VSF header contains L (file length)
    stream.write_all(data)?;
    stream.flush()?;

    Ok(())
}

/// The whole file's length from the first bytes of a VSF file (magic, then z, y, b, L): what a stream carrying one VSF file is framed by.
fn vsf_file_length(header: &[u8]) -> std::io::Result<usize> {
    let bad = |what: String| std::io::Error::new(std::io::ErrorKind::InvalidData, what);
    if header.len() < 4 || &header[0..3] != "RÅ".as_bytes() || header[3] != b'<' {
        return Err(bad("Invalid VSF magic number".into()));
    }
    let mut ptr = 4;
    for field in ["version", "backward compat", "header length"] {
        vsf::decoding::parse(header, &mut ptr).map_err(|e| bad(format!("TCP recv: Failed to parse {}: {}", field, e)))?;
    }
    match vsf::decoding::parse(header, &mut ptr) {
        Ok(vsf::VsfType::l(len, _)) => Ok(len),
        Ok(other) => Err(bad(format!("TCP recv: Expected L for file length, got {:?}", other))),
        Err(e) => Err(bad(format!("TCP recv: Failed to parse L field: {}", e))),
    }
}

/// Receive VSF data from TCP by parsing L from header Returns the complete VSF file bytes
pub fn recv(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    // First, read magic + enough for header fields (conservatively 64 bytes)
    let mut header_buf = vec![0u8; 64];
    stream.read_exact(&mut header_buf)?;
    let file_length = vsf_file_length(&header_buf)?;

    // Sanity check - max 64MB
    if file_length > 64 * 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("VSF file too large: {} bytes", file_length),
        ));
    }

    // Now read the remaining bytes (we already have 64)
    // WHY: `file_length` is the SENDER's header claim — only its upper bound was checked above.
    // PROOF: a claim shorter than the 64 bytes already read needs nothing more; a plain subtraction would wrap and try to read ~2^64.
    let remaining = file_length.saturating_sub(header_buf.len());
    let mut data = header_buf;
    if remaining > 0 {
        data.resize(file_length, 0);
        stream.read_exact(&mut data[64..])?;
    } else {
        // File was smaller than 64 bytes - truncate
        data.truncate(file_length);
    }

    #[cfg(feature = "development")]
    {
        let addr = stream.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "unknown".to_string());
        let msg = vsf_inspect(&data, "TCP", "RX", &addr);
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

/// Async TCP send for PT fallback (fully async, no blocking) No external framing - VSF header contains L (file length)
#[cfg(target_os = "ferros")]
pub async fn send_tcp(_data: &[u8], _addr: SocketAddr) -> std::io::Result<()> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "no TCP on ferros yet"))
}

#[cfg(not(target_os = "ferros"))]
pub async fn send_tcp(data: &[u8], addr: SocketAddr) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    use tokio::time::timeout;

    let connect_timeout = std::time::Duration::from_secs(10);
    let write_timeout = std::time::Duration::from_secs(30);

    // Connect with timeout
    let mut stream = timeout(connect_timeout, tokio::net::TcpStream::connect(addr))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "TCP connect timeout"))??;

    // No length prefix - VSF header contains L (file length) Write with timeout
    timeout(write_timeout, async {
        stream.write_all(data).await?;
        stream.flush().await
    })
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "TCP write timeout"))?
}

/// Send a framed CLUTCH message Format: [payload_type:1][handle_proof:32][payload]
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

/// Parse a received CLUTCH frame Returns (payload_type, handle_proof, payload) if valid
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
#[cfg(not(any(target_os = "android", target_os = "ferros")))]
pub struct TcpListener {
    inner: tokio::net::TcpListener,
}

#[cfg(not(any(target_os = "android", target_os = "ferros")))]
impl TcpListener {
    /// Bind to address
    pub async fn bind(addr: SocketAddr) -> std::io::Result<Self> {
        let inner = tokio::net::TcpListener::bind(addr).await?;
        crate::logf!("TCP: Listening on {}", addr);
        Ok(Self { inner })
    }

    /// Accept incoming connection
    pub async fn accept(&self) -> std::io::Result<(TcpStream, SocketAddr)> {
        let (stream, addr) = self.inner.accept().await?;
        crate::logf!("TCP: Connection from {}", addr);

        // Convert to std TcpStream for blocking I/O
        let std_stream = stream.into_std()?;
        Ok((std_stream, addr))
    }
}

/// One VSF file from an accepted connection, read without blocking a thread: its header's L frames it, and a file claiming more than `cap` bytes is refused before anything past the header is read.
/// The listener runs this per connection under a timeout, so a slow or silent sender holds only its own task (2026-11-26: the accept loop read every connection inline with a std stream left non-blocking, so large frames, CLUTCH included, failed on WouldBlock).
#[cfg(not(any(target_os = "android", target_os = "ferros")))]
pub async fn recv_async(stream: &mut tokio::net::TcpStream, cap: usize) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut data = vec![0u8; 64];
    stream.read_exact(&mut data).await?;
    let file_length = vsf_file_length(&data)?;
    if file_length > cap {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("VSF file too large: {} bytes (cap {})", file_length, cap)));
    }
    // WHY: `file_length` is the SENDER's header claim, bounded above just now.
    // PROOF: a claim shorter than the 64 bytes already read needs nothing more, and a plain subtraction would wrap.
    if file_length > data.len() {
        let have = data.len();
        data.resize(file_length, 0);
        stream.read_exact(&mut data[have..]).await?;
    } else {
        data.truncate(file_length);
    }
    Ok(data)
}
