//! A socket ring on ferros (the ip server's ops 15 RING OPEN and 16 RING CLOSE; the layout is ferros_netring in the ferros repo, docs/WIFI.md "Datagrams at speed"): a run of shared memory with a send ring Photon fills and ip drains and a receive ring ip fills and Photon drains, so a datagram costs no call each way (docs/ferros-media.md "Network").
//! The application's side is restated here rather than taken by path: Photon's lockfile spans every target, so a path into the ferros repo would make every desktop, Android and contributor build read it.
//! The layout's first word is the drift tripwire: a run whose magic is not exactly [`MAGIC`] (ferros bumps its last byte on any change to the layout or the waiting words) is refused, and the socket stays on ip's calls.
//! Every word is a u64 at an 8-byte aligned offset, loaded and stored whole (single-copy atomic on aarch64), the ordering made by fences, as ferros_netring does it.

use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{fence, AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Waker;

/// The run's first word: "fnetrng" and the layout's version, 0.
const MAGIC: u64 = u64::from_le_bytes(*b"fnetrng\x00");
/// The header's bytes, before the send ring.
const HEAD: usize = 256;
const W_MAGIC: usize = 0;
const W_TX_OFF: usize = 8;
const W_TX_LEN: usize = 16;
const W_RX_OFF: usize = 24;
const W_RX_LEN: usize = 32;
/// Photon's line: bytes written into the send ring, bytes taken from the receive ring, the reader's waiting word, the writer's.
const W_TX_HEAD: usize = 64;
const W_RX_TAIL: usize = 72;
const W_APP_WAITS: usize = 80;
const W_APP_ROOM: usize = 88;
/// ip's line: bytes taken from the send ring, bytes written into the receive ring, whether it sleeps, the ring's state.
const W_TX_TAIL: usize = 128;
const W_RX_HEAD: usize = 136;
const W_IP_WAITS: usize = 144;
const W_STATE: usize = 168;
/// A waiting word's value while its side waits; the ring's state while ip serves it.
const WAITING: u64 = 1;
const OPEN: u64 = 0;
/// A record's header: [len u16][family u8][class u8][port u16][flags u16] in one word, the stamp, then the address (16 bytes; IPv4 in the first 4), then the payload padded to a word.
const REC_HEAD: usize = 32;
/// The longest payload a record carries: what a UDP datagram can be.
const PAYLOAD_MOST: usize = 65507;

/// A datagram's class: how ip marks it (DSCP) and which of the WiFi chip's queues it rides.
pub const CLASS_BULK: u8 = 0;
pub const CLASS_VIDEO: u8 = 1;
pub const CLASS_VOICE: u8 = 2;

/// The rings asked for: 2^16 bytes to send (a beam's burst of slices), 2^17 to receive (what lands while the reader is busy, at a beam's rate).
/// ip feeds them while a person is present, which is the identity unlocked (ferros fact 83), not the screen: a wave at the ear keeps both directions; a locked identity gets nothing on any path (ferros AUTH.md "Radios while locked").
const TX_LOG: u8 = 16;
const RX_LOG: u8 = 17;

const SYS_WAIT: u64 = 16;
const SYS_WAKE: u64 = 17;
const SYS_SHARE: u64 = 49;
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

/// A record's bytes in the ring for a payload of `len` bytes.
const fn rec_bytes(len: usize) -> usize {
    REC_HEAD + (len + 7) / 8 * 8
}

/// One open ring: the run's address and bytes, the view the kernel gave it, where its rings lie, Photon's two positions (each under the one side that moves it), and the tasks waiting for a datagram.
pub struct Ring {
    base: u64,
    view: u64,
    port: u16,
    tx_off: usize,
    tx_len: usize,
    rx_off: usize,
    rx_len: usize,
    tx_head: Mutex<u64>,
    rx_tail: Mutex<u64>,
    wakers: Mutex<Vec<Waker>>,
    /// The doorbell thread, for an async reader: started once, stopped before the run goes back to ip.
    bell: Mutex<Option<std::thread::JoinHandle<()>>>,
    stop: AtomicBool,
}

impl Ring {
    fn word(&self, at: usize) -> &AtomicU64 {
        // SAFETY: the run is mapped read-write for this process while it holds the view (ferros docs/KERNEL.md "Shared regions"), `at` is an aligned offset inside it (the header's, or the rings' checked against the run's bytes at open), and the view is let go only in Drop, after every user of this Ring is gone.
        unsafe { &*((self.base + at as u64) as *const AtomicU64) }
    }

    fn ld(&self, at: usize) -> u64 {
        self.word(at).load(Ordering::Relaxed)
    }

    fn st(&self, at: usize, v: u64) {
        self.word(at).store(v, Ordering::Relaxed)
    }

    /// RING OPEN on `port` at the ip server (`call` is the ip endpoint's), the run mapped and checked: None when ip has no rings (an older image), refused, or laid the run out in a shape this copy does not know.
    pub fn open(port: u16, call: impl Fn(&[u8], &mut [u8]) -> Option<usize>) -> Option<Arc<Ring>> {
        let mut r = [0u8; 17];
        let n = call(&[15, (port >> 8) as u8, port as u8, TX_LOG, RX_LOG], &mut r)?;
        if n < 17 || r[0] != 0 {
            crate::logf!("ferros net: RING OPEN on port {} refused (status {}, {} bytes): the socket stays on ip's calls", port, r[0], n);
            return None;
        }
        let view = u64::from_be_bytes(r[1..9].try_into().unwrap());
        let mut b = [0u8; 16];
        let base = svc(SYS_SHARE, 3, view, (b.as_mut_ptr() as u64).wrapping_sub(arc_base()), 0, 0);
        if base == ERR {
            crate::logf!("ferros net: the ring's view on port {} is not ours", port);
            return None;
        }
        let bytes = u64::from_le_bytes(b[..8].try_into().unwrap()) as usize;
        // SAFETY: the kernel just mapped `bytes` bytes at `base` for this process (SHARE VIEW); its first word is the run's.
        let first = unsafe { &*((base + W_MAGIC as u64) as *const AtomicU64) }.load(Ordering::Relaxed);
        let give_back = |why: &str| {
            crate::logf!("ferros net: the ring on port {} {}: the socket stays on ip's calls", port, why);
            let mut req = vec![16u8];
            req.extend_from_slice(&view.to_be_bytes());
            let mut a = [0u8; 4];
            call(&req, &mut a);
        };
        if first != MAGIC {
            give_back("is laid out in a version this Photon does not know");
            return None;
        }
        fence(Ordering::Acquire);
        // SAFETY: as `first`; each header word is inside the run's first page.
        let w = |at: usize| unsafe { &*((base + at as u64) as *const AtomicU64) }.load(Ordering::Relaxed) as usize;
        let (tx_off, tx_len, rx_off, rx_len) = (w(W_TX_OFF), w(W_TX_LEN), w(W_RX_OFF), w(W_RX_LEN));
        // WHY the check: the offsets are ip's words, and every load and store below lands at them; one outside the run would touch memory that is not the run.
        let fits = |off: usize, len: usize| len.is_power_of_two() && len >= 8 && off >= HEAD && off % 8 == 0 && off <= bytes && len <= bytes - off;
        if !fits(tx_off, tx_len) || !fits(rx_off, rx_len) {
            give_back("has rings outside its run");
            return None;
        }
        let ring = Ring {
            base,
            view,
            port,
            tx_off,
            tx_len,
            rx_off,
            rx_len,
            tx_head: Mutex::new(w(W_TX_HEAD) as u64),
            rx_tail: Mutex::new(w(W_RX_TAIL) as u64),
            wakers: Mutex::new(Vec::new()),
            bell: Mutex::new(None),
            stop: AtomicBool::new(false),
        };
        crate::logf!("ferros net: port {} on a ring ({} KiB out, {} KiB in)", port, tx_len >> 10, rx_len >> 10);
        Some(Arc::new(ring))
    }

    pub fn closed(&self) -> bool {
        self.ld(W_STATE) != OPEN
    }

    /// One datagram into the send ring, waking ip when it sleeps. A full ring waits for room up to `room_deadline` on the counter (ip takes a datagram the pass it sees it, so a wait is a pass long), then says WouldBlock, as a full socket buffer does.
    pub fn send(&self, data: &[u8], to: SocketAddr, class: u8, room_deadline: u64) -> io::Result<usize> {
        if data.len() > PAYLOAD_MOST || rec_bytes(data.len()) > self.tx_len {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "longer than a datagram"));
        }
        let (fam, addr) = super::ferros_net::family_and_bytes(to.ip());
        let need = rec_bytes(data.len()) as u64;
        let mut head = self.tx_head.lock().unwrap();
        loop {
            if self.closed() {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "ip let the ring go"));
            }
            let tail = self.ld(W_TX_TAIL);
            fence(Ordering::Acquire);
            // ip's word: a tail past the head would be ip's error, read as no room.
            let used = head.wrapping_sub(tail);
            if used <= self.tx_len as u64 && self.tx_len as u64 - used >= need {
                break;
            }
            // The writer's waiting word, then one more look (the Dekker pair with ip's store of its tail).
            self.st(W_APP_ROOM, WAITING);
            fence(Ordering::SeqCst);
            let tail = self.ld(W_TX_TAIL);
            let used = head.wrapping_sub(tail);
            if used > self.tx_len as u64 || self.tx_len as u64 - used < need {
                svc(SYS_WAIT, (self.base + W_TX_TAIL as u64).wrapping_sub(arc_base()), tail, room_deadline, 1, 0);
            }
            self.st(W_APP_ROOM, 0);
            if super::ferros_net::now_counter() >= room_deadline {
                return Err(io::Error::new(io::ErrorKind::WouldBlock, "the send ring is full"));
            }
        }
        let at = |k: u64| self.tx_off + ((*head + k) as usize & (self.tx_len - 1));
        let hw = data.len() as u64 | (fam as u64) << 16 | (class as u64) << 24 | (to.port() as u64) << 32;
        self.st(at(0), hw);
        self.st(at(8), super::ferros_net::now_counter());
        self.st(at(16), u64::from_le_bytes(addr[..8].try_into().unwrap()));
        self.st(at(24), u64::from_le_bytes(addr[8..].try_into().unwrap()));
        let mut k = REC_HEAD as u64;
        let mut c = data.chunks_exact(8);
        for w in &mut c {
            self.st(at(k), u64::from_le_bytes(w.try_into().unwrap()));
            k += 8;
        }
        let rest = c.remainder();
        if !rest.is_empty() {
            let mut w = [0u8; 8];
            w[..rest.len()].copy_from_slice(rest);
            self.st(at(k), u64::from_le_bytes(w));
        }
        *head += need;
        fence(Ordering::Release);
        self.st(W_TX_HEAD, *head);
        // The Dekker pair with ip's sleep: the head stored before ip's waiting word is read.
        fence(Ordering::SeqCst);
        if self.ld(W_IP_WAITS) != 0 {
            svc(SYS_WAKE, (self.base + W_TX_HEAD as u64).wrapping_sub(arc_base()), 1, 0, 0, 0);
        }
        Ok(data.len())
    }

    /// The next datagram into `buf`, or None when nothing waits. A datagram longer than `buf` is cut to it and the rest dropped, as a UDP socket does.
    pub fn recv(&self, buf: &mut [u8]) -> io::Result<Option<(usize, SocketAddr)>> {
        let mut tail = self.rx_tail.lock().unwrap();
        let head = self.ld(W_RX_HEAD);
        fence(Ordering::Acquire);
        if head == *tail {
            return if self.closed() { Err(io::Error::new(io::ErrorKind::BrokenPipe, "ip let the ring go")) } else { Ok(None) };
        }
        let at = |k: u64| self.rx_off + ((*tail + k) as usize & (self.rx_len - 1));
        let hw = self.ld(at(0));
        let len = (hw & 0xFFFF) as usize;
        let fam = (hw >> 16) as u8;
        let port = (hw >> 32) as u16;
        let mut addr = [0u8; 16];
        addr[..8].copy_from_slice(&self.ld(at(16)).to_le_bytes());
        addr[8..].copy_from_slice(&self.ld(at(24)).to_le_bytes());
        let n = len.min(buf.len());
        let mut k = REC_HEAD as u64;
        let mut c = buf[..n].chunks_exact_mut(8);
        for w in &mut c {
            w.copy_from_slice(&self.ld(at(k)).to_le_bytes());
            k += 8;
        }
        let rest = c.into_remainder();
        if !rest.is_empty() {
            let m = rest.len();
            rest.copy_from_slice(&self.ld(at(k)).to_le_bytes()[..m]);
        }
        *tail += rec_bytes(len) as u64;
        fence(Ordering::Release);
        self.st(W_RX_TAIL, *tail);
        Ok(Some((n, SocketAddr::new(super::ferros_net::ip_from(fam, &addr), port))))
    }

    /// Whether a datagram waits, for a task that registered its waker before looking (so the doorbell's wake is never lost between the look and the registration).
    pub fn ready(&self) -> bool {
        self.ld(W_RX_HEAD) != *self.rx_tail.lock().unwrap() || self.closed()
    }

    /// A blocking reader's wait for a datagram, to `deadline` on the counter (u64::MAX: none): the reader's waiting word, one more look, then the wait on ip's head.
    pub fn wait(&self, deadline: u64) {
        let tail = *self.rx_tail.lock().unwrap();
        self.st(W_APP_WAITS, WAITING);
        fence(Ordering::SeqCst);
        let head = self.ld(W_RX_HEAD);
        if head == tail && !self.closed() {
            let until = if deadline == u64::MAX { 0 } else { deadline };
            svc(SYS_WAIT, (self.base + W_RX_HEAD as u64).wrapping_sub(arc_base()), head, until, 1, 0);
        }
        self.st(W_APP_WAITS, 0);
    }

    /// Register `w` to be woken at the next datagram, the doorbell thread started on first use.
    pub fn register(self: &Arc<Self>, w: &Waker) {
        {
            let mut ws = self.wakers.lock().unwrap();
            if !ws.iter().any(|k| k.will_wake(w)) {
                ws.push(w.clone());
            }
        }
        let mut bell = self.bell.lock().unwrap();
        if bell.is_none() {
            let me = Arc::downgrade(self);
            let name = format!("ring-{}", self.port);
            *bell = std::thread::Builder::new().name(name).spawn(move || doorbell(me)).ok();
        }
    }
}

/// The async reader's doorbell: asleep on ip's head while it stands, and each time it moves the waiting tasks are woken. It holds the ring only weakly between waits and ends when the ring stops or is dropped.
fn doorbell(ring: std::sync::Weak<Ring>) {
    let mut seen = match ring.upgrade() {
        Some(r) => r.ld(W_RX_HEAD).wrapping_sub(1),
        None => return,
    };
    loop {
        let Some(r) = ring.upgrade() else { return };
        if r.stop.load(Ordering::Acquire) {
            return;
        }
        r.st(W_APP_WAITS, WAITING);
        fence(Ordering::SeqCst);
        let head = r.ld(W_RX_HEAD);
        if head == seen && !r.closed() {
            let (word, base) = (W_RX_HEAD, r.base);
            drop(r);
            svc(SYS_WAIT, (base + word as u64).wrapping_sub(arc_base()), head, 0, 1, 0);
            continue;
        }
        r.st(W_APP_WAITS, 0);
        seen = head;
        let woken: Vec<Waker> = std::mem::take(&mut *r.wakers.lock().unwrap());
        for w in woken {
            w.wake();
        }
        if r.closed() {
            return;
        }
    }
}

impl Drop for Ring {
    /// The doorbell stopped and joined first (its wait woken on the word it sleeps on), then the run given back to ip: nothing touches the run once the view is gone.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        svc(SYS_WAKE, (self.base + W_RX_HEAD as u64).wrapping_sub(arc_base()), u64::MAX, 0, 0, 0);
        // The doorbell may itself hold the last reference (the socket dropped while it ran): then it is the thread dropping, past its last touch of the run, and joining itself would never return.
        if let Some(h) = self.bell.get_mut().unwrap().take() {
            if h.thread().id() != std::thread::current().id() {
                drop(h.join());
            }
        }
        let mut req = vec![16u8];
        req.extend_from_slice(&self.view.to_be_bytes());
        let mut a = [0u8; 4];
        super::ferros_net::call(&req, &mut a).ok();
    }
}
