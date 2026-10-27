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

    /// A std socket cannot be made on ferros, so nothing can be converted from one.
    pub fn from_std(_s: std::net::UdpSocket) -> io::Result<Self> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "no std sockets on ferros: bind through platform::net::UdpSocket"))
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
        let (fam, ip) = family_and_bytes(addr.ip());
        let mut req = Vec::with_capacity(22 + data.len());
        req.push(1);
        req.push(fam);
        req.extend_from_slice(&ip);
        req.extend_from_slice(&addr.port().to_be_bytes());
        req.extend_from_slice(&self.port.to_be_bytes());
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
        let (fam, ip) = family_and_bytes(group);
        let mut req = Vec::with_capacity(18);
        req.push(8);
        req.push(fam);
        req.extend_from_slice(&ip);
        let mut reply = [0u8; 8];
        call(&req, &mut reply).map(|_| ())
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
