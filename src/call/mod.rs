//! Voice calls (docs/calls.md) — 1:1, fleet-native, wire-invisible.
//!
//! The three planes: SIGNALING rides the friendship lanes as encrypted control rows (a call is indistinguishable from a message on the wire — no relay ever learns a call happened); MEDIA is an ephemeral UDP plane under a basket-derived key ([`keys`]); HISTORY is ordinary rows (missed/completed/duration) plus the optional kept recording on the attachment plane.
//!
//! No timers anywhere: ringing stops on answer/decline/hangup edges, the caller's patience is the timeout, and the intra-call key ratchet steps on packet COUNT, not clocks.

pub mod engine;
pub mod spool;
pub mod keys;
pub mod packet;
pub mod signal;

use std::net::SocketAddr;
use std::sync::Mutex;

/// The media ingress sink: installed by the call engine at call start, cleared at teardown. The recv worker's two-byte fast path hands matching datagrams here RAW — no PT ack, no StatusUpdate, no parse ladder. `None` (no live call) means media datagrams silently drop, which is also the correct answer for stragglers after hangup.
static MEDIA_SINK: Mutex<Option<std::sync::mpsc::Sender<(Vec<u8>, SocketAddr)>>> = Mutex::new(None);

pub fn install_media_sink(tx: std::sync::mpsc::Sender<(Vec<u8>, SocketAddr)>) {
    *MEDIA_SINK.lock().unwrap() = Some(tx);
}

pub fn clear_media_sink() {
    *MEDIA_SINK.lock().unwrap() = None;
}

/// Called from the recv worker for every magic-matched datagram. Cheap when idle (one mutex + None).
pub fn deliver_media(bytes: &[u8], src: SocketAddr) {
    let sink = MEDIA_SINK.lock().unwrap();
    if let Some(tx) = sink.as_ref() {
        let _ = tx.send((bytes.to_vec(), src));
    }
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

/// The one live call (v1: singular — a second inbound offer during any phase gets an automatic `Busy`).
pub struct ActiveCall {
    pub call_id: [u8; 16],
    /// The friend on the other end (their handle hash — the contact key).
    pub peer_handle_hash: [u8; 32],
    pub we_are_caller: bool,
    pub phase: CallPhase,
    /// Eagle osc at the current phase's start (answer re-stamps it — the duration base).
    pub phase_osc: i64,
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
}
