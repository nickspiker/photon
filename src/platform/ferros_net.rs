//! Photon's sockets on ferros: a `UdpSocket` with tokio's calls over the kernel's `ip` server (payloads/ip in the ferros repo — op 1 SEND, 2 RECV, 3 INFO, 8 JOIN), and the TCP types as uninhabited stand-ins until the ip server's TCP (ops 4-7) is wired. The process reaches the ip server through the grant slot `UDP_SLOT`, fixed by the spawn line in init's shell table (payloads/init); the syscall is inlined, as kete and fluor do, since ferros_sys is not a published crate.
//!
//! A receive polls the server and sleeps 2 ms between empty polls: the ip server keeps datagrams for the last eight ports asked for, and tokio's time driver carries the wait.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Mutex;

/// The grant slot of the `udp` endpoint: slot 0 is the console, display, touch and the app surface come first, udp fourth (std/photon/src/main.rs in the ferros repo).
pub const UDP_SLOT: u64 = 4;

const CALL: u64 = 1;
const ERR: u64 = u64::MAX;

#[inline(always)]
fn svc(n: u64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> u64 {
    let r: u64;
    unsafe { core::arch::asm!("svc #0", inlateout("x0") a0 => r, in("x1") a1, in("x2") a2, in("x3") a3, in("x4") a4, in("x8") n, options(nostack)) };
    r
}

#[inline(always)]
fn arc_base() -> u64 {
    let b: u64;
    unsafe { core::arch::asm!("mov {}, x28", out(reg) b, options(nomem, nostack, preserves_flags)) };
    b
}

fn off<T: ?Sized>(p: *const T) -> u64 {
    (p as *const u8 as u64).wrapping_sub(arc_base())
}

fn cap(slot: u64) -> u64 {
    4 + 32 * slot
}

/// One call to the ip server: the reply's length, or an io error when the kernel refused the call.
fn call(req: &[u8], reply: &mut [u8]) -> io::Result<usize> {
    let n = svc(CALL, cap(UDP_SLOT), off(req.as_ptr()), req.len() as u64, off(reply.as_mut_ptr()), reply.len() as u64);
    if n == ERR { Err(io::Error::new(io::ErrorKind::Other, "the ip server refused the call")) } else { Ok(n as usize) }
}

fn family_and_bytes(ip: IpAddr) -> (u8, [u8; 16]) {
    let mut b = [0u8; 16];
    match ip {
        IpAddr::V4(a) => {
            b[..4].copy_from_slice(&a.octets());
            (4, b)
        }
        IpAddr::V6(a) => {
            b.copy_from_slice(&a.octets());
            (6, b)
        }
    }
}

fn ip_from(family: u8, b: &[u8]) -> IpAddr {
    if family == 4 { IpAddr::V4(Ipv4Addr::new(b[0], b[1], b[2], b[3])) } else { IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&b[..16]).unwrap())) }
}

/// A UDP socket: a port on the ip server.
pub struct UdpSocket {
    port: u16,
    peer: Mutex<Option<SocketAddr>>,
}

impl UdpSocket {
    /// Open `addr`'s port (the address is the device's one address; 0 picks a port from the dynamic range).
    pub async fn bind(addr: SocketAddr) -> io::Result<Self> {
        let port = if addr.port() == 0 { 49152 + (ferros_counter() % 16384) as u16 } else { addr.port() };
        let s = UdpSocket { port, peer: Mutex::new(None) };
        // A RECV opens the port on the server side.
        let mut reply = [0u8; 64];
        let req = [2u8, (port >> 8) as u8, port as u8];
        call(&req, &mut reply)?;
        Ok(s)
    }

    /// The blocking socket's port becomes this async one's, as tokio's `from_std` takes a std socket over.
    pub fn from_std(s: StdUdpSocket) -> io::Result<Self> {
        Ok(UdpSocket { port: s.port, peer: Mutex::new(*s.peer.lock().unwrap()) })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        let mut r = [0u8; 64];
        let n = call(&[3], &mut r)?;
        if n < 55 {
            return Err(io::Error::new(io::ErrorKind::Other, "the ip server's INFO reply is short"));
        }
        let ip = if r[54] & 1 != 0 { IpAddr::V4(Ipv4Addr::new(r[6], r[7], r[8], r[9])) } else { IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&r[22..38]).unwrap())) };
        Ok(SocketAddr::new(ip, self.port))
    }

    pub async fn connect(&self, addr: SocketAddr) -> io::Result<()> {
        *self.peer.lock().unwrap() = Some(addr);
        Ok(())
    }

    pub async fn send_to(&self, data: &[u8], addr: SocketAddr) -> io::Result<usize> {
        send_datagram(self.port, data, addr)
    }

    pub async fn send(&self, data: &[u8]) -> io::Result<usize> {
        let peer = self.peer.lock().unwrap().ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "no peer connected"))?;
        self.send_to(data, peer).await
    }

    /// The next datagram on the port: `(length, sender)`; waits while there is none.
    pub async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let req = [2u8, (self.port >> 8) as u8, self.port as u8];
        let mut reply = vec![0u8; 21 + buf.len().min(65536)];
        loop {
            let n = call(&req, &mut reply)?;
            if n >= 21 {
                let from = SocketAddr::new(ip_from(reply[0], &reply[1..17]), u16::from_be_bytes([reply[17], reply[18]]));
                let len = (n - 21).min(buf.len());
                buf[..len].copy_from_slice(&reply[21..21 + len]);
                return Ok((len, from));
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    }

    pub async fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.recv_from(buf).await.map(|(n, _)| n)
    }

    /// Broadcast is always on at the ip server.
    pub fn set_broadcast(&self, _on: bool) -> io::Result<()> {
        Ok(())
    }

    pub fn join_multicast_v4(&self, group: &Ipv4Addr, _iface: &Ipv4Addr) -> io::Result<()> {
        self.join(IpAddr::V4(*group))
    }

    pub fn join_multicast_v6(&self, group: &Ipv6Addr, _iface: u32) -> io::Result<()> {
        self.join(IpAddr::V6(*group))
    }

    fn join(&self, group: IpAddr) -> io::Result<()> {
        join_group(group)
    }
}

/// A port from the dynamic range for an unnamed bind: the counter's low bits, a different one each call.
fn ferros_counter() -> u64 {
    let t: u64;
    unsafe { core::arch::asm!("mrs {}, cntvct_el0", out(reg) t, options(nomem, nostack, preserves_flags)) };
    t
}

/// TCP over the ip server (ops 4-7) is not wired yet: these types have no values, so the code that would use them is unreachable and compiles.
pub enum TcpListener {}
pub enum TcpStream {}

impl TcpListener {
    pub async fn bind(_addr: SocketAddr) -> io::Result<Self> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "no TCP on ferros yet"))
    }

    pub async fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        match *self {}
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        match *self {}
    }
}

impl TcpStream {
    pub fn into_std(self) -> io::Result<std::net::TcpStream> {
        match self {}
    }
}

/// The blocking shape of a UDP socket (std's), for the threads that talk without tokio: the query worker, the LAN announcer. The same port on the ip server as the async type; `UdpSocket::from_std` hands it over, as tokio's does.
pub struct StdUdpSocket {
    port: u16,
    peer: Mutex<Option<SocketAddr>>,
    read_timeout: Mutex<Option<std::time::Duration>>,
}

impl StdUdpSocket {
    pub fn bind<A: std::net::ToSocketAddrs>(addr: A) -> io::Result<Self> {
        let addr = addr.to_socket_addrs()?.next().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no address to bind"))?;
        let port = if addr.port() == 0 { 49152 + (ferros_counter() % 16384) as u16 } else { addr.port() };
        let mut reply = [0u8; 64];
        call(&[2, (port >> 8) as u8, port as u8], &mut reply)?;
        Ok(StdUdpSocket { port, peer: Mutex::new(None), read_timeout: Mutex::new(None) })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        let mut r = [0u8; 64];
        let n = call(&[3], &mut r)?;
        if n < 55 {
            return Err(io::Error::new(io::ErrorKind::Other, "the ip server's INFO reply is short"));
        }
        let ip = if r[54] & 1 != 0 { IpAddr::V4(Ipv4Addr::new(r[6], r[7], r[8], r[9])) } else { IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&r[22..38]).unwrap())) };
        Ok(SocketAddr::new(ip, self.port))
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(StdUdpSocket { port: self.port, peer: Mutex::new(*self.peer.lock().unwrap()), read_timeout: Mutex::new(*self.read_timeout.lock().unwrap()) })
    }

    pub fn set_read_timeout(&self, t: Option<std::time::Duration>) -> io::Result<()> {
        *self.read_timeout.lock().unwrap() = t;
        Ok(())
    }

    pub fn set_write_timeout(&self, _t: Option<std::time::Duration>) -> io::Result<()> {
        Ok(())
    }

    /// Non-blocking is the ip server's only mode; a receive with no datagram waiting answers WouldBlock when this is set.
    pub fn set_nonblocking(&self, on: bool) -> io::Result<()> {
        if on {
            *self.read_timeout.lock().unwrap() = Some(std::time::Duration::ZERO);
        }
        Ok(())
    }

    pub fn set_broadcast(&self, _on: bool) -> io::Result<()> {
        Ok(())
    }

    /// The ip server sends multicast at its own hop limit and does not loop it back.
    pub fn set_multicast_ttl_v4(&self, _ttl: u32) -> io::Result<()> {
        Ok(())
    }

    pub fn set_multicast_loop_v4(&self, _on: bool) -> io::Result<()> {
        Ok(())
    }

    pub fn set_multicast_loop_v6(&self, _on: bool) -> io::Result<()> {
        Ok(())
    }

    pub fn set_ttl(&self, _ttl: u32) -> io::Result<()> {
        Ok(())
    }

    pub fn connect<A: std::net::ToSocketAddrs>(&self, addr: A) -> io::Result<()> {
        *self.peer.lock().unwrap() = addr.to_socket_addrs()?.next();
        Ok(())
    }

    pub fn send_to<A: std::net::ToSocketAddrs>(&self, data: &[u8], addr: A) -> io::Result<usize> {
        let addr = addr.to_socket_addrs()?.next().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no address to send to"))?;
        send_datagram(self.port, data, addr)
    }

    pub fn send(&self, data: &[u8]) -> io::Result<usize> {
        let peer = self.peer.lock().unwrap().ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "no peer connected"))?;
        send_datagram(self.port, data, peer)
    }

    /// The next datagram, waiting up to the read timeout (none set: as long as it takes), 2 ms between empty polls of the ip server.
    pub fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let timeout = *self.read_timeout.lock().unwrap();
        let start = std::time::Instant::now();
        let req = [2u8, (self.port >> 8) as u8, self.port as u8];
        let mut reply = vec![0u8; 21 + buf.len().min(65536)];
        loop {
            let n = call(&req, &mut reply)?;
            if n >= 21 {
                let from = SocketAddr::new(ip_from(reply[0], &reply[1..17]), u16::from_be_bytes([reply[17], reply[18]]));
                let len = (n - 21).min(buf.len());
                buf[..len].copy_from_slice(&reply[21..21 + len]);
                return Ok((len, from));
            }
            if let Some(t) = timeout {
                if start.elapsed() >= t {
                    return Err(io::Error::new(if t.is_zero() { io::ErrorKind::WouldBlock } else { io::ErrorKind::TimedOut }, "no datagram"));
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    pub fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.recv_from(buf).map(|(n, _)| n)
    }

    pub fn join_multicast_v4(&self, group: &Ipv4Addr, _iface: &Ipv4Addr) -> io::Result<()> {
        join_group(IpAddr::V4(*group))
    }

    pub fn join_multicast_v6(&self, group: &Ipv6Addr, _iface: u32) -> io::Result<()> {
        join_group(IpAddr::V6(*group))
    }
}

/// One datagram out through the ip server (op 1 SEND).
fn send_datagram(from_port: u16, data: &[u8], addr: SocketAddr) -> io::Result<usize> {
    let (fam, ip) = family_and_bytes(addr.ip());
    let mut req = Vec::with_capacity(22 + data.len());
    req.push(1);
    req.push(fam);
    req.extend_from_slice(&ip);
    req.extend_from_slice(&addr.port().to_be_bytes());
    req.extend_from_slice(&from_port.to_be_bytes());
    req.extend_from_slice(data);
    let mut reply = [0u8; 16];
    let n = call(&req, &mut reply)?;
    match (n, reply[0]) {
        (0, _) => Err(io::Error::new(io::ErrorKind::Other, "the ip server said nothing")),
        (_, 0) => Ok(data.len()),
        (_, 1) => Err(io::Error::new(io::ErrorKind::WouldBlock, "resolving the next hop")),
        (_, 2) => Err(io::Error::new(io::ErrorKind::AddrNotAvailable, "no address yet")),
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "the ip server refused the datagram")),
    }
}

/// Join a multicast group at the ip server (op 8 JOIN): it reports the membership so a snooping access point forwards the group.
fn join_group(group: IpAddr) -> io::Result<()> {
    let (fam, ip) = family_and_bytes(group);
    let mut req = Vec::with_capacity(18);
    req.push(8);
    req.push(fam);
    req.extend_from_slice(&ip);
    let mut reply = [0u8; 8];
    call(&req, &mut reply).map(|_| ())
}
