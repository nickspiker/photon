//! Photon's sockets on ferros: a `UdpSocket` with tokio's calls over the kernel's `ip` server (payloads/ip in the ferros repo — op 1 SEND, 2 RECV, 3 INFO, 8 JOIN, 11 WATCH, 12 KICK), and the TCP types as uninhabited stand-ins until the ip server's TCP (ops 4-7) is wired. The process reaches the ip server through the grant slot `UDP_SLOT`, fixed by the spawn line in init's shell table (payloads/init); the syscall is inlined, as kete and fluor do, since ferros_sys is not a published crate.
//!
//! A receive never polls (ferros owner, 2026-11-10: "No polling!"). The async socket takes what is waiting (RECV), and when nothing is it waits on the process's one watch at the ip server (op 11 WATCH, `watcher`): a thread holds a call there naming every bound port, answered when a datagram comes for one of them or the network's flags change, and wakes the tasks waiting on those ports. The blocking socket's receive is held at the ip server to its read timeout exactly (op 2 with a deadline). Until 2026-11-13 both asked RECV every 2 ms a socket, about a thousand calls a second from an idle Photon on the 8a; an ip server from before WATCH still gets that.

use std::future::Future;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

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

/// Grant `slot`'s token: past the handle table (512 bytes) and the u32 count at G#200 (ferros SANDBOX.md, "Handles", since 2026-11-03).
fn cap(slot: u64) -> u64 {
    0x204 + 32 * slot
}

/// One call to the ip server: the reply's length, or an io error when the kernel refused the call.
pub(crate) fn call(req: &[u8], reply: &mut [u8]) -> io::Result<usize> {
    let n = svc(CALL, cap(UDP_SLOT), off(req.as_ptr()), req.len() as u64, off(reply.as_mut_ptr()), reply.len() as u64);
    if n == ERR { Err(io::Error::new(io::ErrorKind::Other, "the ip server refused the call")) } else { Ok(n as usize) }
}

/// The counter the kernel's deadlines are on (`cntvct_el0`), and its ticks a second.
fn counter() -> u64 {
    let t: u64;
    unsafe { core::arch::asm!("isb", "mrs {}, cntvct_el0", out(reg) t, options(nomem, nostack, preserves_flags)) };
    t
}

fn counter_hz() -> u64 {
    let f: u64;
    unsafe { core::arch::asm!("mrs {}, cntfrq_el0", out(reg) f, options(nomem, nostack, preserves_flags)) };
    f
}

/// `d` from now on the counter: the deadline the ip server holds a call to.
fn deadline_after(d: std::time::Duration) -> u64 {
    let t = counter() as u128 + d.as_nanos() * counter_hz() as u128 / 1_000_000_000;
    // WHY the check: the timeout is the caller's, and Duration reaches 2^64 s; past the counter's range (about 24,000 years at 24 MHz) cast down it would wrap to a deadline already passed and time out at once. A timeout that long is none: u64::MAX, held until a datagram comes.
    if t >= u64::MAX as u128 { u64::MAX } else { t as u64 }
}

/// One WATCH (op 11): [11, mode, flags seen, id, datagram seen, deadline, ports] → (flags, the datagram told of up to, the ports with news), or None from an ip server without the op.
fn watch_call(level: bool, flags: u8, id: u32, seen: u32, deadline: u64, ports: &[u16]) -> Option<(u8, u32, Vec<u16>)> {
    let mut req = Vec::with_capacity(19 + 2 * ports.len());
    req.extend_from_slice(&[11, level as u8, flags]);
    req.extend_from_slice(&id.to_be_bytes());
    req.extend_from_slice(&seen.to_be_bytes());
    req.extend_from_slice(&deadline.to_be_bytes());
    for p in ports {
        req.extend_from_slice(&p.to_be_bytes());
    }
    let mut r = [0u8; 64];
    let n = call(&req, &mut r).ok()?;
    // An answer is 6 bytes and two a port named; anything else (an ip server's [3] for an op it lacks) is no watch.
    if n < 6 {
        return None;
    }
    // WHY the min: the count is the server's word; only the ports the reply holds are read.
    let k = (r[5] as usize).min((n.min(r.len()) - 6) >> 1);
    let named = (0..k).map(|i| u16::from_be_bytes([r[6 + 2 * i], r[7 + 2 * i]])).collect();
    Some((r[0], u32::from_be_bytes([r[1], r[2], r[3], r[4]]), named))
}

/// A port the watcher names: the sockets on it, the news it has had (a count a waiting task compares), the tasks waiting on it.
struct Watched {
    port: u16,
    users: u32,
    news: u64,
    wakers: Vec<Waker>,
}

/// The process's one watch at the ip server: the ports named, the flags last said (0xFF none yet), the set's generation and the one the held call carries (None between calls), the watcher thread started, and `none` while the server answers no WATCH (receives then poll every 2 ms, as before).
struct Watcher {
    ports: Vec<Watched>,
    flags: u8,
    gen: u64,
    held: Option<u64>,
    started: bool,
    none: bool,
}

static WATCHER: Mutex<Watcher> = Mutex::new(Watcher { ports: Vec::new(), flags: 0xFF, gen: 0, held: None, started: false, none: false });

/// The watch's id at the ip server (KICK names it): the counter's bits and the arc's, one a process.
fn watch_id() -> u32 {
    static ID: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *ID.get_or_init(|| (counter() as u32) ^ ((arc_base() >> 12) as u32).rotate_left(16))
}

/// A blocking socket's reader id: the process's id with a sequence number a socket folded in, so no two of this process's sockets (nor one and the watcher, number 0) share one.
fn reader_id() -> u32 {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    watch_id() ^ NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed).rotate_left(16)
}

/// A socket on `port`: the port named in the watch from now on. A port new to it while the watcher's call is held is told with a KICK (op 12), which ends that call so the next names it.
fn watch_add(port: u16) {
    let kick = {
        let mut w = WATCHER.lock().unwrap();
        if let Some(p) = w.ports.iter_mut().find(|p| p.port == port) {
            p.users += 1;
            false
        } else {
            w.ports.push(Watched { port, users: 1, news: 0, wakers: Vec::new() });
            w.gen += 1;
            if !w.started {
                w.started = true;
                std::thread::Builder::new().name("ip-watch".into()).spawn(watcher).expect("the ip watcher thread");
            }
            w.held.is_some_and(|g| g != w.gen)
        }
    };
    if kick {
        let mut r = [0u8; 8];
        let mut req = vec![12u8];
        req.extend_from_slice(&watch_id().to_be_bytes());
        let _ = call(&req, &mut r);
    }
}

/// A socket on `port` gone: the port left out of the next watch once no socket is on it (no KICK: a port watched for nothing costs a wakeup at most).
fn watch_drop(port: u16) {
    let mut w = WATCHER.lock().unwrap();
    if let Some(i) = w.ports.iter().position(|p| p.port == port) {
        w.ports[i].users -= 1;
        if w.ports[i].users == 0 {
            for k in w.ports.swap_remove(i).wakers {
                k.wake();
            }
            w.gen += 1;
        }
    }
}

/// The watcher thread: one WATCH held at the ip server naming every port, answered when a datagram comes for one (the tasks waiting on it woken), when the flags change (every task woken: a person back, a route come or gone), or at a KICK (a port added: the next call names it). The datagram it was told of goes back as seen, so one a task has not read yet never answers again. Nothing on a clock.
fn watcher() {
    let id = watch_id();
    let mut seen = 0u32;
    // The port set's generation last said to be past sixteen: said once a change, not once a call.
    let mut said: Option<u64> = None;
    loop {
        let (ports, flags) = {
            let mut w = WATCHER.lock().unwrap();
            w.held = Some(w.gen);
            let mut ports: Vec<u16> = w.ports.iter().map(|p| p.port).collect();
            // The server watches sixteen ports a call (payloads/ip WATCH_PORTS); Photon binds a handful. A seventeenth would never wake its receives: said loudly, once each time the set changes while past sixteen.
            if ports.len() > 16 {
                if said != Some(w.gen) {
                    said = Some(w.gen);
                    crate::logf!("ferros net: {} ports bound, the ip server watches 16 a call: the receives on port {} and those after it will not wake", ports.len(), ports[16]);
                    debug_assert!(false, "more than 16 ports watched at the ip server");
                }
                ports.truncate(16);
            }
            (ports, w.flags)
        };
        // Level mode on the first watch (no flags seen yet): every datagram waiting counts, the seen number meaning nothing yet.
        let answer = watch_call(flags == 0xFF, flags, id, seen, 0, &ports);
        let mut w = WATCHER.lock().unwrap();
        w.held = None;
        let Some((flags, upto, named)) = answer else {
            // No watch at this server: every receive back to asking every 2 ms, and the watch tried again 4 s on (WHY a timer: only a restarted ip server can answer it, and nothing rings for that).
            w.none = true;
            w.flags = 0xFF;
            for p in w.ports.iter_mut() {
                p.news += 1;
                for k in p.wakers.drain(..) {
                    k.wake();
                }
            }
            drop(w);
            std::thread::sleep(std::time::Duration::from_secs(4));
            continue;
        };
        w.none = false;
        seen = upto;
        let all = flags != w.flags;
        w.flags = flags;
        for p in w.ports.iter_mut().filter(|p| all || named.contains(&p.port)) {
            p.news += 1;
            for k in p.wakers.drain(..) {
                k.wake();
            }
        }
    }
}

/// The news count of `port` before a receive looks, or None while the server has no watch.
fn watch_news(port: u16) -> Option<u64> {
    let w = WATCHER.lock().unwrap();
    if w.none {
        return None;
    }
    Some(w.ports.iter().find(|p| p.port == port).map_or(0, |p| p.news))
}

/// Ready once `port` has had news since `seen` (the count before the receive that found nothing), so a datagram that came between that receive and this wait is never slept through.
struct News {
    port: u16,
    seen: u64,
}

impl Future for News {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut w = WATCHER.lock().unwrap();
        if w.none {
            return Poll::Ready(());
        }
        // A socket registers its port when made and drops it when dropped, so a receive on a port the watcher does not know is a bug here: said, and pending (Ready would spin the task on its RECV).
        let Some(p) = w.ports.iter_mut().find(|p| p.port == self.port) else {
            crate::logf!("ferros net: a receive waits on port {} the watcher does not know", self.port);
            debug_assert!(false, "receive on an unregistered port");
            return Poll::Pending;
        };
        if p.news != self.seen {
            return Poll::Ready(());
        }
        if !p.wakers.iter().any(|k| k.will_wake(cx.waker())) {
            p.wakers.push(cx.waker().clone());
        }
        Poll::Pending
    }
}

/// One RECV on `port` (op 2), as reader `id` (a watch's: the server keeps each datagram of a port several processes watch until each has taken it, so fgtw's copy of a fleet frame is not Photon's to take), the datagram into `buf`: Some((length, sender)), or None when nothing waits. `deadline`: on the counter, the server holds the call to it while nothing waits (0 at once, u64::MAX until a datagram comes).
fn recv_once(port: u16, id: u32, buf: &mut [u8], deadline: u64) -> io::Result<Option<(usize, SocketAddr)>> {
    let mut req = vec![2u8, (port >> 8) as u8, port as u8, 0];
    req.extend_from_slice(&deadline.to_be_bytes());
    req.extend_from_slice(&id.to_be_bytes());
    let mut reply = vec![0u8; 21 + buf.len().min(65536)];
    let n = call(&req, &mut reply)?;
    if n < 21 {
        return Ok(None);
    }
    let from = SocketAddr::new(ip_from(reply[0], &reply[1..17]), u16::from_be_bytes([reply[17], reply[18]]));
    // WHY the mins: the length is the server's word, the buffers are ours.
    let len = (n.min(reply.len()) - 21).min(buf.len());
    buf[..len].copy_from_slice(&reply[21..21 + len]);
    Ok(Some((len, from)))
}

/// Open `port` at the ip server (a RECV opens it, at once) as reader `id`: a datagram no reader waits for is taken and dropped (one that came before the socket), never another reader's copy (fgtw's on 4383 and 4384).
fn open_port(port: u16, id: u32) -> io::Result<()> {
    let mut buf = [0u8; 0];
    recv_once(port, id, &mut buf, 0).map(|_| ())
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
        open_port(port, watch_id())?;
        watch_add(port);
        Ok(UdpSocket { port, peer: Mutex::new(None) })
    }

    /// The blocking socket's port becomes this async one's, as tokio's `from_std` takes a std socket over.
    pub fn from_std(s: StdUdpSocket) -> io::Result<Self> {
        watch_add(s.port);
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

    /// The next datagram on the port: `(length, sender)`; waits while there is none, on the watch (`News`), never on a clock. Cancel-safe as tokio's is: a dropped wait has taken nothing.
    pub async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        loop {
            // The port's news count before the RECV: news after it ends the wait at once.
            let seen = watch_news(self.port);
            if let Some(got) = recv_once(self.port, watch_id(), buf, 0)? {
                return Ok(got);
            }
            match seen {
                Some(seen) => News { port: self.port, seen }.await,
                // An ip server without WATCH: asked again 2 ms on, as before.
                None => tokio::time::sleep(std::time::Duration::from_millis(2)).await,
            }
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

impl Drop for UdpSocket {
    fn drop(&mut self) {
        watch_drop(self.port);
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
    /// Its reader id at the ip server (its own watch's, apart from the process's watcher's, so the two never answer each other's calls); a socket made async reads as the process's watcher.
    id: u32,
    /// Whether this socket has watched at the server (its blocking receive waited out an early answer): told UNWATCH (op 13) when dropped, so no datagram waits for a reader that is gone.
    watched: std::sync::atomic::AtomicBool,
    peer: Mutex<Option<SocketAddr>>,
    read_timeout: Mutex<Option<std::time::Duration>>,
}

impl Drop for StdUdpSocket {
    /// A socket that watched tells the server its reader is gone (op 13 UNWATCH), so no datagram of its port waits for it; one that never watched has nothing there to forget. (A process that ends without dropping is forgotten by the server 64 s after its last watch.)
    fn drop(&mut self) {
        if self.watched.load(std::sync::atomic::Ordering::Relaxed) {
            let mut req = vec![13u8];
            req.extend_from_slice(&self.id.to_be_bytes());
            let mut r = [0u8; 8];
            crate::log_err!(call(&req, &mut r), "ferros net: UNWATCH of a dropped socket");
        }
    }
}

impl StdUdpSocket {
    pub fn bind<A: std::net::ToSocketAddrs>(addr: A) -> io::Result<Self> {
        let addr = addr.to_socket_addrs()?.next().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no address to bind"))?;
        let id = reader_id();
        // A named port is a listening one: a RECV opens it on the server side now, so what comes before the first receive is kept. An unnamed one is opened by its first send or receive: the routing look-ups (`get_local_ip_toward`) bind one, connect and read the address, and opened at the bind each took one of the server's eight kept ports from a socket that listens.
        let port = if addr.port() == 0 {
            49152 + (ferros_counter() % 16384) as u16
        } else {
            open_port(addr.port(), id)?;
            addr.port()
        };
        Ok(StdUdpSocket { port, id, watched: false.into(), peer: Mutex::new(None), read_timeout: Mutex::new(None) })
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

    /// The clone reads under an id of its own: two blocking receives on one port are two readers, each its own copy of each datagram once it watches, and dropping one never retires the other (UNWATCH names the id).
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(StdUdpSocket { port: self.port, id: reader_id(), watched: false.into(), peer: Mutex::new(*self.peer.lock().unwrap()), read_timeout: Mutex::new(*self.read_timeout.lock().unwrap()) })
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

    /// The next datagram, waiting up to the read timeout (none set: as long as it takes). The wait is the ip server's: the RECV held there to the timeout's deadline exactly (op 2 with a deadline), answered as a datagram lands. A hold answered early with nothing (nobody present, or the server's sixteen holds taken) waits on a watch of this port and the flags to the same deadline (op 11), so an absence is slept through, not asked about.
    pub fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let timeout = *self.read_timeout.lock().unwrap();
        if timeout == Some(std::time::Duration::ZERO) {
            return recv_once(self.port, self.id, buf, 0)?.ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, "no datagram"));
        }
        // u64::MAX: no timeout, held until a datagram comes.
        let deadline = timeout.map_or(u64::MAX, deadline_after);
        // The flags the server last said (0xFF none yet: the first watch is answered at once with them, and every later one holds while they stand).
        let mut flags = 0xFFu8;
        loop {
            if let Some(got) = recv_once(self.port, self.id, buf, deadline)? {
                return Ok(got);
            }
            if counter() >= deadline {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "no datagram"));
            }
            // Answered early with nothing: nobody present (the watch holds until the person bit changes), or the server's sixteen holds taken and this one the oldest (the watch holds in a table of its own). The watch's deadline: 0 is none at the server.
            let until = if deadline == u64::MAX { 0 } else { deadline };
            self.watched.store(true, std::sync::atomic::Ordering::Relaxed);
            match watch_call(true, flags, self.id, 0, until, &[self.port]) {
                Some((f, _, _)) => flags = f,
                // An ip server without WATCH (nor deadlines): asked again 2 ms on, as before.
                None => std::thread::sleep(std::time::Duration::from_millis(2)),
            }
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
