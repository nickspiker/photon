//! Voice calls (docs/calls.md) — 1:1, fleet-native, wire-invisible.
//!
//! The three planes: SIGNALING rides the friendship lanes as encrypted control rows (a call is indistinguishable from a message on the wire — no relay ever learns a call happened); MEDIA is an ephemeral UDP plane under a basket-derived key ([`keys`]); HISTORY is ordinary rows (missed/completed/duration) plus the optional kept recording on the attachment plane.
//!
//! No timers anywhere: ringing stops on answer/decline/hangup edges, the caller's patience is the timeout, and the intra-call key ratchet steps on packet COUNT, not clocks.

pub mod engine;
pub mod spool;
pub mod record;
pub mod playback;
pub mod keys;
pub mod packet;
pub mod signal;

use std::net::SocketAddr;
use std::sync::Mutex;

/// The media ingress sink: installed by the call engine at call start, cleared at teardown. The recv worker's two-byte fast path hands matching datagrams here RAW — no PT ack, no StatusUpdate, no parse ladder. `None` (no live call) means media datagrams silently drop, which is also the correct answer for stragglers after hangup.
static MEDIA_SINK: Mutex<Option<std::sync::mpsc::Sender<(Vec<u8>, SocketAddr)>>> = Mutex::new(None);

/// True exactly while a call engine is up (sink installed → cleared) — the "be quiet, media is flowing" signal for background chatter (discovery beacons, history walks) that shares the socket/recv path with the 50pps media stream. Engine lifecycle, not audio-session: recording playback never sets it.
pub static MEDIA_QUIET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn install_media_sink(tx: std::sync::mpsc::Sender<(Vec<u8>, SocketAddr)>) {
    *MEDIA_SINK.lock().unwrap() = Some(tx);
    MEDIA_QUIET.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn clear_media_sink() {
    *MEDIA_SINK.lock().unwrap() = None;
    MEDIA_QUIET.store(false, std::sync::atomic::Ordering::Relaxed);
}

/// Called from the recv worker for every magic-matched datagram. Cheap when idle (one mutex + None).
pub fn deliver_media(bytes: &[u8], src: SocketAddr) {
    let sink = MEDIA_SINK.lock().unwrap();
    if let Some(tx) = sink.as_ref() {
        let _ = tx.send((bytes.to_vec(), src));
    }
}

/// Express-signal ingress (signal.rs EXPRESS frames): the recv worker parks magic-matched datagrams here raw; the UI tick drains + trial-opens them against its friendships (it owns the keys). Bounded — signals are rare, and anything beyond the cap is flood noise.
static EXPRESS_RX: Mutex<Vec<(Vec<u8>, SocketAddr)>> = Mutex::new(Vec::new());
const EXPRESS_RX_CAP: usize = 32;

/// Recv-worker side: park one express frame for the UI drain. Cheap, lock-push-unlock.
pub fn deliver_express(bytes: &[u8], src: SocketAddr) {
    let mut q = EXPRESS_RX.lock().unwrap();
    if q.len() < EXPRESS_RX_CAP {
        q.push((bytes.to_vec(), src));
    }
}

/// UI-tick side: take everything parked since the last drain.
pub fn take_express_frames() -> Vec<(Vec<u8>, SocketAddr)> {
    std::mem::take(&mut *EXPRESS_RX.lock().unwrap())
}

/// Media EGRESS: packets must leave from the MAIN UDP socket (the port the peer's NAT knows), so the engine hands them to a dedicated tokio forwarder inside the network runtime — installed once at checker startup.
static MEDIA_TX: Mutex<Option<tokio::sync::mpsc::UnboundedSender<(Vec<u8>, SocketAddr)>>> =
    Mutex::new(None);

pub fn install_media_tx(tx: tokio::sync::mpsc::UnboundedSender<(Vec<u8>, SocketAddr)>) {
    *MEDIA_TX.lock().unwrap() = Some(tx);
}

/// Engine-side send. False when the network runtime is gone (shutdown) — the engine treats that as a stop edge.
pub fn send_media(bytes: Vec<u8>, addr: SocketAddr) -> bool {
    let tx = MEDIA_TX.lock().unwrap();
    match tx.as_ref() {
        Some(t) => t.send((bytes, addr)).is_ok(),
        None => false,
    }
}

/// Where a call stands. The phases are edges, not timers: Outgoing ends on answer/decline/busy or OUR hangup (the caller's patience is the timeout); Ringing ends on local answer/decline, a sibling's answer, or the caller's hangup; Active ends on either side's hangup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallPhase {
    /// We sent the offer; nothing answered yet.
    Outgoing,
    /// Their offer reached us; we are ringing.
    Ringing,
    /// Media flowing.
    Active,
    /// Hung up, recording decision pending: the bar shows Keep / Delete (docs/calls.md — recording by default, endpoint memory).
    Ended,
}

/// Silences the desktop ring loop when dropped (or explicitly). The loop thread replays the relationship ring cadence (`chirp::Chirp::ring_from_hash`) until this flag flips; holding the guard inside [`ActiveCall`] makes every teardown edge — decline, sibling answer, caller hangup, call overwrite — a ring-stop edge for free, honoring the no-timers rule (the flag IS an edge).
pub struct RingGuard(pub std::sync::Arc<std::sync::atomic::AtomicBool>);

impl Drop for RingGuard {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The one live call (v1: singular — a second inbound offer during any phase gets an automatic `Busy`).
pub struct ActiveCall {
    pub call_id: [u8; 16],
    /// The friend on the other end (their handle hash — the contact key).
    pub peer_handle_hash: [u8; 32],
    pub we_are_caller: bool,
    pub phase: CallPhase,
    /// Eagle osc at the current phase's start (answer re-stamps it — the duration base).
    pub phase_osc: i64,
    /// Eagle osc frozen at the Active→Ended edge, so the end-screen call-duration summary doesn't keep growing while the Keep/Delete decision is open. `None` until hangup. A single stamp at the end edge — no timer.
    pub final_osc: Option<i64>,
    /// The offer row's eagle stamp — identical on BOTH fleets (it's the row's wire timestamp), so summary rows minted independently at offer_osc+1 dedup across every device.
    pub offer_osc: i64,
    pub caller_nonce: [u8; 32],
    pub callee_nonce: Option<[u8; 32]>,
    /// The lane key the offer row was sealed under — the doomed egg (keys.rs). Captured at the send COMMIT (caller, via drain_braid_tx matching the offer content) or at decrypt (callee, pre-advance).
    pub offer_lane_key: Option<[u8; 32]>,
    /// The basket-derived call secret, once both nonces exist. The media engine builds its StepChains from this; teardown drops it (RAM only, never persisted).
    pub secret: Option<[u8; 32]>,
    /// The running media engine (Active phase). Teardown = explicit `stop()` — the thread zeroizes its chains and releases audio on exit.
    pub engine: Option<engine::EngineHandle>,
    /// The recording's keep/delete material (Active → Ended). Dropping it undecided IS the shred — the key lives nowhere else.
    pub spool: Option<spool::SpoolTicket>,
    /// Desktop ring-loop stopper (Ringing phase only; `None` on Android — Kotlin owns playback there). Dropped or cleared = ring stops at the next cadence boundary.
    pub ring: Option<RingGuard>,
    /// The source address the peer's most recent EXPRESS signal arrived from — the freshest known direct path to the device actually driving this call. Express replies target it first (answer rides back the offer's path), beside the contact's validated path.
    pub express_addr: Option<SocketAddr>,
}
