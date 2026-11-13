//! Plain HTTP from ferros: one request a connection over the ip server's client TCP (ferros payloads/ip, the `udp` endpoint at `UDP_SLOT`: 4 CONNECT, 5 SEND, 6 RECV, 7 CLOSE) and the device's one resolver (10 RESOLVE).
//! ferros has no TLS, and fgtw.org answers plain HTTP on port 80 as it answers HTTPS (ferros memory fgtw_plain_http_for_ferros): every FGTW call is a POST of a signed VSF document, folded and verified on this side, so the clear costs no integrity.
//! Every wait is a call the ip server holds until something comes (the wait byte of ops 6 and 10), never a poll; the two deadlines are real ones: a name nobody answers, and a server that goes silent mid-reply.

use super::ferros_net::call;

/// How long the resolver may take for a name (its own tries are four, a second apart).
const RESOLVE_WAIT_S: u8 = 1 << 3;
/// How long a connection may bring nothing before it is given up (WHY a deadline: ip fails a connection only when what it sent goes unanswered, and a reply's body is all receiving; a server gone silent mid-reply would hold the caller for ever). Photon's other platforms give FGTW 15 s.
const STALL_S: u8 = 1 << 4;
/// The most one TCP receive asks for: 256 KiB, all one reply of the ip server carries (its RECV_MAX); the buffer holds the state byte ahead of it.
const RECV_MOST: usize = 1 << 18;
/// The most of a request one SEND carries: a call carries 4096 bytes (the kernel's limit), less the op and the handle.
const SEND_MAX: usize = (1 << 12) - 2;
/// The most of a reply taken: 16 MiB (WHY: the length is the server's word; a reply past it is refused, never grown into).
const REPLY_MOST: usize = 1 << 24;

/// POST `body` to `http://{host}:{port}{path}`: the reply's status and body, or why not in a few plain words.
pub fn post(host: &str, port: u16, path: &str, content_type: &str, body: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let (family, address) = resolve(host)?;
    let conn = Conn::open(family, address, port)?;
    let mut request = format!("POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
    request.extend_from_slice(body);
    conn.send_all(&request)?;
    conn.reply()
}

/// `host`'s address from the ip server's resolver: (family, address with IPv4 in its first 4 bytes).
fn resolve(host: &str) -> Result<(u8, [u8; 16]), String> {
    let mut req = vec![10u8, 0, RESOLVE_WAIT_S];
    req.extend_from_slice(host.as_bytes());
    let mut r = [0u8; 32];
    let n = call(&req, &mut r).map_err(|e| e.to_string())?;
    // WHY the length check: the reply is another process's; 22 bytes is a whole answer, anything shorter (nothing, while nobody is present) is no address.
    if n < 22 {
        return Err("the resolver answered nothing (nobody present)".to_string());
    }
    match r[0] {
        0 => Ok((r[1], r[2..18].try_into().unwrap())),
        1 => Err("the name is still being asked for".to_string()),
        2 => Err("the name has no address".to_string()),
        4 => Err("no way to ask for the name (no network)".to_string()),
        5 => Err("nobody answered for the name".to_string()),
        _ => Err("the resolver refused the name".to_string()),
    }
}

/// One connection on the ip server, closed when dropped.
struct Conn {
    handle: u8,
}

impl Conn {
    fn open(family: u8, address: [u8; 16], port: u16) -> Result<Conn, String> {
        let mut req = vec![4u8, family];
        req.extend_from_slice(&address);
        req.extend_from_slice(&port.to_be_bytes());
        let mut r = [0u8; 16];
        let n = call(&req, &mut r).map_err(|e| e.to_string())?;
        match (n, r[0]) {
            (2.., 0) => Ok(Conn { handle: r[1] }),
            (_, 2) => Err("no address yet".to_string()),
            (_, 4) => Err("the ip server has no free connection".to_string()),
            (_, 6) => Err("the ip server has no memory for another connection".to_string()),
            _ => Err("the ip server refused the connection".to_string()),
        }
    }

    /// The whole request into the connection's send buffer (256 KiB, empty on a fresh connection, which the ip server sends as the peer's window allows). A buffer that takes nothing is an error, never a retry loop: no request of Photon's comes near it.
    fn send_all(&self, mut data: &[u8]) -> Result<(), String> {
        while !data.is_empty() {
            let part = &data[..data.len().min(SEND_MAX)];
            let mut req = Vec::with_capacity(2 + part.len());
            req.extend_from_slice(&[5, self.handle]);
            req.extend_from_slice(part);
            let mut r = [0u8; 16];
            let n = call(&req, &mut r).map_err(|e| e.to_string())?;
            let took = match (n, r[0]) {
                (3.., 0) => u16::from_be_bytes([r[1], r[2]]) as usize,
                _ => return Err("the connection failed before the request was sent".to_string()),
            };
            if took == 0 {
                return Err("the connection's send buffer is full".to_string());
            }
            data = &data[took..];
        }
        Ok(())
    }

    /// The reply, read as it comes: done once its length is in (Content-Length, or a chunked body's last chunk), or the server closes.
    fn reply(&self) -> Result<(u16, Vec<u8>), String> {
        let mut ask = [6, self.handle, STALL_S, 0, 0, 0, 0];
        ask[3..7].copy_from_slice(&(RECV_MOST as u32).to_be_bytes());
        let mut rx = vec![0u8; 1 + RECV_MOST];
        let mut raw = Vec::new();
        let mut seen = 1u8;
        loop {
            // Held until data, a change of the connection's state (it opened, the server closed, it failed), or STALL_S with nothing.
            let n = call(&ask, &mut rx).map_err(|e| e.to_string())?;
            // WHY the min: the length is the kernel's, never past the buffer it was given.
            let n = n.min(rx.len());
            if n == 0 {
                return Err("the ip server answered nothing (nobody present)".to_string());
            }
            let state = rx[0];
            match state {
                4 => return Err("the connection failed (reset, or unanswered)".to_string()),
                5 => return Err("the connection is gone".to_string()),
                _ => {}
            }
            if n > 1 {
                raw.extend_from_slice(&rx[1..n]);
                if raw.len() > REPLY_MOST {
                    return Err("the reply is larger than any FGTW answer".to_string());
                }
                seen = state;
                if let Some(done) = parse(&raw, false)? {
                    return Ok(done);
                }
                continue;
            }
            if state == 3 {
                return parse(&raw, true)?.ok_or_else(|| "the reply ended early".to_string());
            }
            // The connection opened (a new state with nothing yet): wait again. The same state with nothing: the wait ran out.
            if state != seen {
                seen = state;
                continue;
            }
            return Err("the server went silent".to_string());
        }
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        let mut r = [0u8; 4];
        // A close the ip server refuses leaves nothing to do here: it reaps a connection its app has not asked about for 32 s.
        crate::log_err!(call(&[7, self.handle], &mut r), "FGTW: closing the HTTP connection");
    }
}

/// An HTTP/1.1 reply read so far: Some((status, body)) once whole, None while more is to come (`closed`: the server has said everything, so a reply still short is an error). Every read is checked (WHY: the bytes are the network's, any length and any content).
fn parse(raw: &[u8], closed: bool) -> Result<Option<(u16, Vec<u8>)>, String> {
    let Some(end) = find(raw, b"\r\n\r\n") else {
        return if closed { Err("the reply ended inside its head".to_string()) } else { Ok(None) };
    };
    let head = std::str::from_utf8(&raw[..end]).map_err(|_| "the reply's head is not text".to_string())?;
    let mut lines = head.split("\r\n");
    let mut status_line = lines.next().unwrap_or("").split(' ');
    if !status_line.next().is_some_and(|v| v.starts_with("HTTP/1.")) {
        return Err("not an HTTP reply".to_string());
    }
    let status: u16 = status_line.next().and_then(|s| s.parse().ok()).ok_or_else(|| "the reply has no status".to_string())?;
    let (mut length, mut chunked) = (None, false);
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            length = Some(value.parse::<usize>().map_err(|_| "the reply's length is not a number".to_string())?);
        } else if name.eq_ignore_ascii_case("transfer-encoding") && value.to_ascii_lowercase().contains("chunked") {
            chunked = true;
        }
    }
    let body = &raw[end + 4..];
    if chunked {
        return match unchunk(body)? {
            Some(b) => Ok(Some((status, b))),
            None if closed => Err("the reply's chunks ended early".to_string()),
            None => Ok(None),
        };
    }
    match length {
        Some(l) if body.len() >= l => Ok(Some((status, body[..l].to_vec()))),
        Some(_) if closed => Err("the reply ended short of its length".to_string()),
        Some(_) => Ok(None),
        None if closed => Ok(Some((status, body.to_vec()))),
        None => Ok(None),
    }
}

/// A chunked body: Some(the bytes) once its last chunk is in, None while more is to come.
fn unchunk(mut b: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let mut out = Vec::new();
    loop {
        let Some(eol) = find(b, b"\r\n") else { return Ok(None) };
        let line = std::str::from_utf8(&b[..eol]).map_err(|_| "a chunk's size is not text".to_string())?;
        let size = usize::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16).map_err(|_| "a chunk's size is not a number".to_string())?;
        // WHY: the size is the server's word; one past the most a reply may be is refused before anything is added to it.
        if size > REPLY_MOST - out.len() {
            return Err("the reply is larger than any FGTW answer".to_string());
        }
        b = &b[eol + 2..];
        if size == 0 {
            return Ok(Some(out));
        }
        if b.len() < size + 2 {
            return Ok(None);
        }
        out.extend_from_slice(&b[..size]);
        b = &b[size + 2..];
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

