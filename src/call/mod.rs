//! Voice calls (docs/calls.md) — 1:1, fleet-native, wire-invisible.
//!
//! The three planes: SIGNALING rides the friendship lanes as encrypted control rows (a call is indistinguishable from a message on the wire — no relay ever learns a call happened); MEDIA is an ephemeral UDP plane under a basket-derived key ([`keys`]); HISTORY is ordinary rows (missed/completed/duration) plus the optional kept recording on the attachment plane.
//!
//! No timers anywhere: ringing stops on answer/decline/hangup edges, the caller's patience is the timeout, and the intra-call key ratchet steps on packet COUNT, not clocks.

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

/// Where a call stands. The phases are edges, not timers: Outgoing ends on answer/decline/busy or OUR hangup (the caller's patience is the timeout); Ringing ends on local answer/decline, a sibling's answer, or the caller's hangup; Active ends on either side's hangup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallPhase {
    /// We sent the offer; nothing answered yet.
    Outgoing,
    /// Their offer reached us; we are ringing.
    Ringing,
    /// Media flowing.
    Active,
}

/// The one live call (v1: singular — a second inbound offer during any phase gets an automatic `Busy`).
pub struct ActiveCall {
    pub call_id: [u8; 16],
    /// The friend on the other end (their handle hash — the contact key).
    pub peer_handle_hash: [u8; 32],
    pub we_are_caller: bool,
    pub phase: CallPhase,
    /// Eagle osc at offer (Outgoing/Ringing) and re-stamped at answer (Active) — the duration base for the summary row.
    pub phase_osc: i64,
    pub caller_nonce: [u8; 32],
    pub callee_nonce: Option<[u8; 32]>,
    /// The lane key the offer row was sealed under — the doomed egg (keys.rs). Captured at send (caller) or decrypt (callee).
    pub offer_lane_key: [u8; 32],
}
