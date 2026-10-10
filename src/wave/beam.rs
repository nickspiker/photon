//! The BEAM track — video beside the wave's voice (docs/beams.md). The transport only: codec-agnostic, it carries ENCODED frames (H.264 access units) from whoever produces them to whoever decodes them.
//!
//! Built the way the recording-fill plane is built: its own packet magic ([`packet::BEAM_MAGIC`]), its own sequence space, its own [`StepChain`]s from a domain-separated child of the wave secret ([`beam_secret`]), demuxed by magic at the media sink ([`super::deliver_media`]) so the audio engine never sees a beam datagram.
//!
//! One encoded frame = one fountain-code object. It is fragmented into symbols of [`SYMBOL_BYTES`] (MTU-safe with every header on), each symbol SELF-DESCRIBING — frame number, the frame's whole length (the fountain geometry both ends derive from it, no negotiation), the sender's per-frame white-balance gains (docs/beams.md: pre-emphasis applied by the ISP, divided back out on receipt) — so whichever symbol lands first opens the frame, and repair symbols ride in proportion. Arrival order is irrelevant; a frame that never completes is a loss the receiver counts and the next intra-refresh stripe heals (no keyframes, ever — the sender's rule, not the transport's).
//!
//! The beam describes itself IN-BAND: a [`Kind::Info`] packet (geometry, fps, codec, the VSF characterization entry: matrix, class, tier, transfer) goes out at start and once a second, like intra refresh for metadata — nothing is added to the offer/answer, a beam can start mid-wave, a late joiner needs nothing from signalling.
//!
//! Threads: the sender runs on whatever thread produces frames ([`BeamTx::send_frame`] is a plain call); the receiver is one thread fed by the sink, handing completed frames to [`take_frames`] for the decoder stage.

use super::keys::{Direction, StepChain, PACKETS_PER_STEP};
use super::packet;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::mpsc;
use std::sync::Mutex;
use zeroize::Zeroize;

/// One fountain symbol's payload. Sized so a full datagram stays under the IPv6 minimum MTU with every header on: 5 (media header) + 1 (kind) + 4 (frame) + 4 (len) + 8 (gains) + 4 (fountain payload id) + 1160 + 4 (tag) = 1190 < 1232.
pub const SYMBOL_BYTES: u16 = 1160;
/// Repair symbols per frame: one per this many source symbols, and never fewer than one. The loss loop can raise it later; one in eight covers the single-loss case a 1/512 setpoint makes the common one.
pub const REPAIR_PER_SOURCE: usize = 8;
/// Frames held open on the receiver awaiting symbols. Older than this behind the newest completed frame, a frame is given up and counted lost — the intra-refresh stripe will cover it.
pub const RX_HORIZON: u32 = 8;
/// The beam's VSF-free in-band info is re-sent every this many frames by the sender (the caller passes `info_due`; this is the suggested cadence).
pub const INFO_EVERY_FRAMES: u32 = 30;

/// The beam track's root: a domain-separated child of the wave secret, so beam datagrams run their own StepChains and seq space beside audio and fill without ever sharing a key+nonce pair.
pub fn beam_secret(wave_secret: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key("PHOTON_WAVE_v1 beam video", wave_secret)
}

/// What a sealed beam payload carries, its first byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    /// One fountain symbol of an encoded frame.
    Data = 0,
    /// The beam's self-description (geometry, fps, codec, colour).
    Info = 1,
}

/// Which codec the encoded frames are in. One today; the byte exists so a second never needs a flag day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Codec {
    H264 = 1,
}

impl Codec {
    pub fn name(self) -> &'static str {
        match self {
            Codec::H264 => "H.264",
        }
    }
}

/// The beam's in-band self-description. `colour` is the VSF characterization entry's bytes (docs/beams.md §1: the camera→VSF-RGB matrix, `IdtClass`, `ProfileTier`, `Transfer`, provenance), opaque to the transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    pub width: u16,
    pub height: u16,
    pub fps: u8,
    pub codec: Codec,
    pub colour: Vec<u8>,
}

/// The per-frame white-balance gains the sender's ISP applied (R, G-even, G-odd, B) in Q12 — pre-emphasis the receiver divides back out. All 4096 = none applied (a desktop camera, or a phone whose HAL would not take ours).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gains(pub [u16; 4]);

impl Gains {
    pub const NONE: Gains = Gains([4096; 4]);
}

/// One completed encoded frame, as the receiver hands it to the decoder stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub frame_no: u32,
    pub gains: Gains,
    pub bytes: Vec<u8>,
}

// ───────────────────────── wire payloads ─────────────────────────

const DATA_HEAD: usize = 1 + 4 + 4 + 8; // kind, frame_no, len, gains
const INFO_HEAD: usize = 1 + 2 + 2 + 1 + 1; // kind, w, h, fps, codec

fn oti(len: u32) -> raptorq::ObjectTransmissionInformation {
    raptorq::ObjectTransmissionInformation::with_defaults(len as u64, SYMBOL_BYTES)
}

fn encode_info(info: &Info) -> Vec<u8> {
    let mut out = Vec::with_capacity(INFO_HEAD + info.colour.len());
    out.push(Kind::Info as u8);
    out.extend_from_slice(&info.width.to_le_bytes());
    out.extend_from_slice(&info.height.to_le_bytes());
    out.push(info.fps);
    out.push(info.codec as u8);
    out.extend_from_slice(&info.colour);
    out
}

fn decode_info(p: &[u8]) -> Option<Info> {
    if p.len() < INFO_HEAD || p[0] != Kind::Info as u8 {
        return None;
    }
    let codec = match p[6] {
        1 => Codec::H264,
        _ => return None,
    };
    Some(Info {
        width: u16::from_le_bytes([p[1], p[2]]),
        height: u16::from_le_bytes([p[3], p[4]]),
        fps: p[5],
        codec,
        colour: p[INFO_HEAD..].to_vec(),
    })
}

fn data_head(frame_no: u32, len: u32, gains: Gains) -> [u8; DATA_HEAD] {
    let mut h = [0u8; DATA_HEAD];
    h[0] = Kind::Data as u8;
    h[1..5].copy_from_slice(&frame_no.to_le_bytes());
    h[5..9].copy_from_slice(&len.to_le_bytes());
    for (i, g) in gains.0.iter().enumerate() {
        h[9 + i * 2..11 + i * 2].copy_from_slice(&g.to_le_bytes());
    }
    h
}

fn parse_data_head(p: &[u8]) -> Option<(u32, u32, Gains, &[u8])> {
    if p.len() <= DATA_HEAD || p[0] != Kind::Data as u8 {
        return None;
    }
    let frame_no = u32::from_le_bytes([p[1], p[2], p[3], p[4]]);
    let len = u32::from_le_bytes([p[5], p[6], p[7], p[8]]);
    let mut g = [0u16; 4];
    for (i, v) in g.iter_mut().enumerate() {
        *v = u16::from_le_bytes([p[9 + i * 2], p[10 + i * 2]]);
    }
    Some((frame_no, len, Gains(g), &p[DATA_HEAD..]))
}

// ───────────────────────── the sender ─────────────────────────

/// The sending half: fragments, fountain-codes and seals frames toward `peer`. Lives on whatever thread produces frames.
pub struct BeamTx {
    chain: StepChain,
    seq: u32,
    peer: SocketAddr,
    /// Datagrams handed to the socket forwarder; `send_fail` counts the ones it refused (the runtime gone = a stop edge the caller reads).
    pub sent: u64,
    pub send_fail: u64,
}

impl BeamTx {
    pub fn new(wave_secret: &[u8; 32], dir: Direction, peer: SocketAddr) -> Self {
        let mut s = beam_secret(wave_secret);
        let chain = StepChain::new(&s, dir);
        s.zeroize();
        Self { chain, seq: 0, peer, sent: 0, send_fail: 0 }
    }

    /// The peer moved (address-follows-auth, the wave's own redirect): point the beam there too.
    pub fn set_peer(&mut self, peer: SocketAddr) {
        self.peer = peer;
    }

    pub fn peer(&self) -> SocketAddr {
        self.peer
    }

    fn seal_and_send(&mut self, payload: &[u8]) {
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        self.chain.advance_to(StepChain::step_for_seq(seq));
        if let Some(wire) = packet::seal_beam(&self.chain, seq, payload) {
            if super::send_media(wire, self.peer) {
                self.sent += 1;
            } else {
                self.send_fail += 1;
            }
        }
    }

    /// Send the beam's self-description. Call at start and on the `INFO_EVERY_FRAMES` cadence (the caller's frame counter is the edge).
    pub fn send_info(&mut self, info: &Info) {
        let p = encode_info(info);
        self.seal_and_send(&p);
    }

    /// Send one encoded frame: fragment to symbols, add repair in proportion, seal each under this seq, hand every datagram to the socket. Returns the number of datagrams sent (source + repair).
    pub fn send_frame(&mut self, frame_no: u32, gains: Gains, bytes: &[u8]) -> usize {
        if bytes.is_empty() || bytes.len() > u32::MAX as usize {
            return 0;
        }
        let len = bytes.len() as u32;
        let head = data_head(frame_no, len, gains);
        let enc = raptorq::Encoder::new(bytes, oti(len));
        let source_n = (bytes.len() + SYMBOL_BYTES as usize - 1) / SYMBOL_BYTES as usize;
        let repair_n = (source_n + REPAIR_PER_SOURCE - 1) / REPAIR_PER_SOURCE;
        let pkts = enc.get_encoded_packets(repair_n as u32);
        let mut n = 0;
        for pkt in pkts {
            let sym = pkt.serialize();
            let mut payload = Vec::with_capacity(DATA_HEAD + sym.len());
            payload.extend_from_slice(&head);
            payload.extend_from_slice(&sym);
            self.seal_and_send(&payload);
            n += 1;
        }
        n
    }
}

// ───────────────────────── the receiver ─────────────────────────

/// The receiving half: opens beam datagrams, feeds symbols to per-frame fountain decoders, emits completed frames in order of completion, counts what never completed.
pub struct BeamRx {
    chain: StepChain,
    open: BTreeMap<u32, (u32, Gains, raptorq::Decoder)>,
    done: BTreeMap<u32, ()>,
    newest_done: u32,
    /// The latest self-description heard, and whether it changed since the decoder stage last looked.
    info: Option<Info>,
    info_changed: bool,
    pub frames_done: u64,
    pub frames_lost: u64,
    pub bad_packets: u64,
}

impl BeamRx {
    pub fn new(wave_secret: &[u8; 32], dir: Direction) -> Self {
        let mut s = beam_secret(wave_secret);
        let chain = StepChain::new(&s, dir);
        s.zeroize();
        Self {
            chain,
            open: BTreeMap::new(),
            done: BTreeMap::new(),
            newest_done: 0,
            info: None,
            info_changed: false,
            frames_done: 0,
            frames_lost: 0,
            bad_packets: 0,
        }
    }

    pub fn info(&self) -> Option<&Info> {
        self.info.as_ref()
    }

    /// The self-description, iff it changed since the last take (the decoder stage reconfigures on this edge).
    pub fn take_info_change(&mut self) -> Option<Info> {
        if self.info_changed {
            self.info_changed = false;
            self.info.clone()
        } else {
            None
        }
    }

    /// One beam datagram off the wire. Returns a completed frame when this symbol finished one.
    pub fn push(&mut self, wire: &[u8]) -> Option<Frame> {
        let (header, sealed) = match packet::parse_header(wire) {
            Some(h) if h.0.beam => h,
            _ => {
                self.bad_packets += 1;
                return None;
            }
        };
        let payload = match packet::open(&mut self.chain, &header, sealed) {
            Some(p) => p,
            None => {
                self.bad_packets += 1;
                return None;
            }
        };
        match payload.first() {
            Some(&k) if k == Kind::Info as u8 => {
                if let Some(info) = decode_info(&payload) {
                    if self.info.as_ref() != Some(&info) {
                        self.info = Some(info);
                        self.info_changed = true;
                    }
                } else {
                    self.bad_packets += 1;
                }
                None
            }
            Some(&k) if k == Kind::Data as u8 => self.push_symbol(&payload),
            _ => {
                self.bad_packets += 1;
                None
            }
        }
    }

    fn push_symbol(&mut self, payload: &[u8]) -> Option<Frame> {
        let (frame_no, len, gains, sym) = parse_data_head(payload)?;
        if len == 0 {
            self.bad_packets += 1;
            return None;
        }
        if self.done.contains_key(&frame_no) {
            return None; // a late duplicate of a frame already handed out
        }
        if frame_no + RX_HORIZON < self.newest_done {
            return None; // given up on already
        }
        let pkt = raptorq::EncodingPacket::deserialize(sym);
        let entry = self
            .open
            .entry(frame_no)
            .or_insert_with(|| (len, gains, raptorq::Decoder::new(oti(len))));
        if entry.0 != len {
            // Two symbols disagreeing about the frame's length cannot both be this frame; keep the first, drop the stranger.
            self.bad_packets += 1;
            return None;
        }
        let out = entry.2.decode(pkt)?;
        self.open.remove(&frame_no);
        self.done.insert(frame_no, ());
        self.frames_done += 1;
        if frame_no > self.newest_done {
            self.newest_done = frame_no;
        }
        self.reap();
        Some(Frame { frame_no, gains, bytes: out })
    }

    /// Give up frames the horizon has passed: every open frame older than `newest_done − RX_HORIZON` is a loss; every done marker older still is forgotten.
    fn reap(&mut self) {
        let floor = self.newest_done.saturating_sub(RX_HORIZON);
        let stale: Vec<u32> = self.open.range(..floor).map(|(k, _)| *k).collect();
        for k in stale {
            self.open.remove(&k);
            self.frames_lost += 1;
        }
        let forget: Vec<u32> = self.done.range(..floor.saturating_sub(RX_HORIZON)).map(|(k, _)| *k).collect();
        for k in forget {
            self.done.remove(&k);
        }
    }

    /// Frames still open (awaiting symbols) — the instrument line.
    pub fn open_frames(&self) -> usize {
        self.open.len()
    }
}

// ───────────────────────── the sink and the thread ─────────────────────────

/// Beam ingress, demuxed by [`super::deliver_media`] on the magic: installed when a beam receiver starts, cleared when it stops.
static BEAM_SINK: Mutex<Option<mpsc::Sender<Vec<u8>>>> = Mutex::new(None);
/// Completed encoded frames for the decoder stage, newest last. Bounded: a stalled decoder drops the OLDEST (a late frame is worthless; the newest is the picture).
static BEAM_FRAMES: Mutex<Vec<Frame>> = Mutex::new(Vec::new());
/// Signalled on every completed frame, so the decoder stage blocks instead of polling.
static BEAM_FRAMES_CV: std::sync::Condvar = std::sync::Condvar::new();
const FRAMES_CAP: usize = 4;
/// The latest self-description the receiver heard (for the decoder stage's reconfigure edge and the UI's "what is this beam" line).
static BEAM_INFO: Mutex<Option<Info>> = Mutex::new(None);

/// Is a beam receiver up? The media sink asks before demuxing.
pub fn rx_live() -> bool {
    BEAM_SINK.lock().unwrap().is_some()
}

/// Recv-worker side: one beam datagram. Cheap when no receiver is up (one mutex + None).
pub fn deliver(bytes: &[u8]) {
    let sink = BEAM_SINK.lock().unwrap();
    if let Some(tx) = sink.as_ref() {
        let _ = tx.send(bytes.to_vec());
    }
}

/// Decoder-stage side: every frame completed since the last take, oldest first.
pub fn take_frames() -> Vec<Frame> {
    std::mem::take(&mut *BEAM_FRAMES.lock().unwrap())
}

/// The blocking form: waits up to `timeout` for a frame to complete (the frame is the edge; the timeout only lets a stop flag be seen), then takes everything queued.
pub fn take_frames_wait(timeout: std::time::Duration) -> Vec<Frame> {
    let guard = BEAM_FRAMES.lock().unwrap();
    let (mut guard, _) = BEAM_FRAMES_CV.wait_timeout_while(guard, timeout, |q| q.is_empty()).unwrap();
    std::mem::take(&mut *guard)
}

pub fn current_info() -> Option<Info> {
    BEAM_INFO.lock().unwrap().clone()
}

/// A running beam receiver thread; dropping the handle stops it.
pub struct RxHandle {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for RxHandle {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        *BEAM_SINK.lock().unwrap() = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        BEAM_FRAMES.lock().unwrap().clear();
        *BEAM_INFO.lock().unwrap() = None;
    }
}

/// Start the beam receiver for a wave: installs the sink and runs the reassembly thread. `dir` is the direction the PEER sends in (the chain this end opens with).
pub fn start_rx(wave_secret: &[u8; 32], dir: Direction) -> RxHandle {
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    *BEAM_SINK.lock().unwrap() = Some(tx);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop2 = stop.clone();
    let mut brx = BeamRx::new(wave_secret, dir);
    let thread = std::thread::Builder::new()
        .name("beam-rx".into())
        .spawn(move || {
            while !stop2.load(std::sync::atomic::Ordering::Relaxed) {
                // A blocking recv with a timeout is the thread's only wait: datagrams are the edges, the timeout only lets the stop flag be seen.
                match rx.recv_timeout(std::time::Duration::from_millis(250)) {
                    Ok(bytes) => {
                        if let Some(frame) = brx.push(&bytes) {
                            let mut q = BEAM_FRAMES.lock().unwrap();
                            if q.len() >= FRAMES_CAP {
                                q.remove(0);
                            }
                            q.push(frame);
                            BEAM_FRAMES_CV.notify_one();
                        }
                        if let Some(info) = brx.take_info_change() {
                            *BEAM_INFO.lock().unwrap() = Some(info);
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            crate::logf!(
                "BEAM: rx thread down — {} frames done, {} lost, {} bad packets",
                brx.frames_done,
                brx.frames_lost,
                brx.bad_packets
            );
        })
        .expect("beam-rx thread");
    RxHandle { stop, thread: Some(thread) }
}

#[allow(dead_code)]
const _STEP_SANITY: () = assert!(PACKETS_PER_STEP > 0);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wave::keys::derive_wave_secret;

    fn secret() -> [u8; 32] {
        derive_wave_secret(&[9; 32], &[8; 16], &[7; 32], &[6; 32])
    }

    /// A sender that captures its datagrams instead of touching the socket: the seal path, bare.
    fn sealed_datagrams(tx: &mut BeamTx, frame_no: u32, gains: Gains, bytes: &[u8]) -> Vec<Vec<u8>> {
        let len = bytes.len() as u32;
        let head = data_head(frame_no, len, gains);
        let enc = raptorq::Encoder::new(bytes, oti(len));
        let source_n = (bytes.len() + SYMBOL_BYTES as usize - 1) / SYMBOL_BYTES as usize;
        let repair_n = (source_n + REPAIR_PER_SOURCE - 1) / REPAIR_PER_SOURCE;
        let mut out = Vec::new();
        for pkt in enc.get_encoded_packets(repair_n as u32) {
            let sym = pkt.serialize();
            let mut payload = Vec::with_capacity(DATA_HEAD + sym.len());
            payload.extend_from_slice(&head);
            payload.extend_from_slice(&sym);
            let seq = tx.seq;
            tx.seq += 1;
            tx.chain.advance_to(StepChain::step_for_seq(seq));
            out.push(packet::seal_beam(&tx.chain, seq, &payload).unwrap());
        }
        out
    }

    #[test]
    fn a_frame_survives_losing_its_repair_share() {
        let s = secret();
        let peer: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let mut tx = BeamTx::new(&s, Direction::OriginToAnswer, peer);
        let mut rx = BeamRx::new(&s, Direction::OriginToAnswer);
        // A 20 KB frame: 18 source symbols, 3 repair.
        let frame: Vec<u8> = (0..20_000u32).map(|i| (i * 7 % 251) as u8).collect();
        let gains = Gains([5000, 4096, 4096, 7000]);
        let wire = sealed_datagrams(&mut tx, 42, gains, &frame);
        assert_eq!(wire.len(), 18 + 3);
        // Lose two source symbols; the repair covers them.
        let mut got = None;
        for (i, d) in wire.iter().enumerate() {
            if i == 3 || i == 11 {
                continue;
            }
            if let Some(f) = rx.push(d) {
                got = Some(f);
            }
        }
        let f = got.expect("the frame completes from source + repair");
        assert_eq!(f.frame_no, 42);
        assert_eq!(f.gains, gains);
        assert_eq!(f.bytes, frame);
        assert_eq!(rx.frames_done, 1);
        assert_eq!(rx.bad_packets, 0);
        // Late duplicates of a done frame are silently ignored.
        assert!(rx.push(&wire[0]).is_none());
    }

    #[test]
    fn a_frame_the_horizon_passes_is_counted_lost() {
        let s = secret();
        let peer: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let mut tx = BeamTx::new(&s, Direction::AnswerToOrigin, peer);
        let mut rx = BeamRx::new(&s, Direction::AnswerToOrigin);
        let small = vec![1u8; 500];
        // Frame 1 arrives with only half its symbols (two source, no repair reaches): stays open.
        let big: Vec<u8> = vec![2u8; 3000];
        let w1 = sealed_datagrams(&mut tx, 1, Gains::NONE, &big);
        assert!(rx.push(&w1[0]).is_none());
        assert_eq!(rx.open_frames(), 1);
        // Frames up to 1 + RX_HORIZON + 1 complete; the horizon passes frame 1.
        for n in 2..=(2 + RX_HORIZON) {
            let w = sealed_datagrams(&mut tx, n, Gains::NONE, &small);
            assert!(rx.push(&w[0]).is_some(), "a one-symbol frame completes on its first symbol");
        }
        assert_eq!(rx.open_frames(), 0);
        assert_eq!(rx.frames_lost, 1);
    }

    #[test]
    fn info_round_trips_and_changes_are_edges() {
        let s = secret();
        let peer: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let mut tx = BeamTx::new(&s, Direction::OriginToAnswer, peer);
        let mut rx = BeamRx::new(&s, Direction::OriginToAnswer);
        let info = Info { width: 640, height: 480, fps: 30, codec: Codec::H264, colour: b"vsf-characterization-bytes".to_vec() };
        let p = encode_info(&info);
        assert_eq!(decode_info(&p).unwrap(), info);
        tx.chain.advance_to(0);
        let wire = packet::seal_beam(&tx.chain, 0, &p).unwrap();
        assert!(rx.push(&wire).is_none());
        assert_eq!(rx.take_info_change(), Some(info.clone()));
        assert_eq!(rx.take_info_change(), None, "unchanged info is not an edge");
        let wire2 = packet::seal_beam(&tx.chain, 1, &p).unwrap();
        rx.push(&wire2);
        assert_eq!(rx.take_info_change(), None);
    }

    #[test]
    fn the_wrong_direction_and_the_audio_key_both_fail() {
        let s = secret();
        let peer: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let mut tx = BeamTx::new(&s, Direction::OriginToAnswer, peer);
        let w = sealed_datagrams(&mut tx, 1, Gains::NONE, &[3u8; 100]);
        let mut wrong = BeamRx::new(&s, Direction::AnswerToOrigin);
        assert!(wrong.push(&w[0]).is_none());
        assert_eq!(wrong.bad_packets, 1);
        // The audio chain of the same direction must not open a beam datagram either (domain-separated secret).
        let mut audio = StepChain::new(&s, Direction::OriginToAnswer);
        let (h, sealed) = packet::parse_header(&w[0]).unwrap();
        assert!(h.beam);
        assert!(packet::open(&mut audio, &h, sealed).is_none());
    }
}
