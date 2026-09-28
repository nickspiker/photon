//! The LIVE FIELD's data (Nick 2026-09-28): every 5 ms frame of a running wave, reduced to the same four envelope powers the kept card is built from — total, and the red / green / blue voice bands of `record::LiveBands` — so the wave screen can ripple the audio out of each avatar in the recording's own colours.
//!
//! Two rings, TX (the wire frame we send, after the level plan) and RX (each frame as it decodes, under its name), indexed by frame number (grid sample ÷ FRAME).
//! The engine thread writes one entry per frame; the UI thread copies the recent history once per paint. A frame that never arrived is simply absent — it paints as nothing, honestly.

use crate::wave::align::FRAME;
use crate::wave::record::{LiveBands, ENV_COMPONENTS};
use std::sync::Mutex;

/// Frames of history the field shows: 1024 × 5 ms ≈ 5.1 s rippling out of each avatar.
pub const FIELD_FRAMES: usize = 1024;
/// Ring capacity: twice the shown history, a power of two so a frame number maps to its slot by masking.
const RING: usize = FIELD_FRAMES * 2;

/// One frame's envelope: its frame number and mean power per component (fraction of full-scale power). `fno == i64::MIN` = empty slot.
#[derive(Clone, Copy, Debug)]
pub struct FrameEnv {
    pub fno: i64,
    pub p: [f32; ENV_COMPONENTS],
}

impl FrameEnv {
    const EMPTY: FrameEnv = FrameEnv { fno: i64::MIN, p: [0.0; ENV_COMPONENTS] };
}

struct Side {
    ring: Vec<FrameEnv>,
    newest: Option<i64>,
    bands: LiveBands,
}

impl Side {
    fn new() -> Self {
        Side { ring: vec![FrameEnv::EMPTY; RING], newest: None, bands: LiveBands::new() }
    }

    fn push(&mut self, k0: i64, pcm: &[i16]) {
        let fno = k0.div_euclid(FRAME);
        let p = self.bands.frame_powers(pcm);
        // PROOF: RING is a power of two and rem_euclid is non-negative, so the slot is in range for any frame number.
        self.ring[fno.rem_euclid(RING as i64) as usize] = FrameEnv { fno, p };
        self.newest = Some(self.newest.map_or(fno, |n| n.max(fno)));
    }

    /// The last FIELD_FRAMES frames ending at `now`, youngest first; a slot holding another frame number (overwritten, or never written) reads empty.
    fn history(&self, now: i64, out: &mut Vec<FrameEnv>) {
        out.clear();
        out.extend((0..FIELD_FRAMES as i64).map(|age| {
            let fno = now - age;
            let e = self.ring[fno.rem_euclid(RING as i64) as usize];
            if e.fno == fno { e } else { FrameEnv::EMPTY }
        }));
    }
}

struct Live {
    tx: Side,
    rx: Side,
}

static LIVE: Mutex<Option<Live>> = Mutex::new(None);

/// A wave's engine came up: fresh, empty rings (and fresh band filters, so one wave's filter state never bleeds into the next).
pub fn start() {
    *LIVE.lock().unwrap() = Some(Live { tx: Side::new(), rx: Side::new() });
}

/// The engine went down: the field has nothing to show.
pub fn stop() {
    *LIVE.lock().unwrap() = None;
}

/// One TX frame, named by its grid sample `k0`.
pub fn push_tx(k0: i64, pcm: &[i16]) {
    if let Some(l) = LIVE.lock().unwrap().as_mut() {
        l.tx.push(k0, pcm);
    }
}

/// One RX frame as it decodes, under the name of the input it carries.
pub fn push_rx(k0: i64, pcm: &[i16]) {
    if let Some(l) = LIVE.lock().unwrap().as_mut() {
        l.rx.push(k0, pcm);
    }
}

/// Copy the recent history for one paint: TX ending at the newest frame sent, RX ending at the frame the speaker is playing now (`rx_play_k`, a grid sample), or the newest arrival before playout has begun. `false` when no wave is running.
pub fn snapshot(rx_play_k: Option<i64>, tx: &mut Vec<FrameEnv>, rx: &mut Vec<FrameEnv>) -> bool {
    let guard = LIVE.lock().unwrap();
    let Some(l) = guard.as_ref() else {
        return false;
    };
    match l.tx.newest {
        Some(n) => l.tx.history(n, tx),
        None => {
            tx.clear();
            tx.resize(FIELD_FRAMES, FrameEnv::EMPTY);
        }
    }
    match rx_play_k.map(|k| k.div_euclid(FRAME)).or(l.rx.newest) {
        Some(n) => l.rx.history(n, rx),
        None => {
            rx.clear();
            rx.resize(FIELD_FRAMES, FrameEnv::EMPTY);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frames land by name, history reads youngest first, a missing frame reads empty, and a slot reused by a later frame never impersonates the older one.
    #[test]
    fn history_is_by_name_with_honest_gaps() {
        let mut s = Side::new();
        let tone: Vec<i16> = (0..FRAME).map(|i| if i % 2 == 0 { 8000 } else { -8000 }).collect();
        for f in [10i64, 11, 13] {
            s.push(f * FRAME, &tone);
        }
        let mut h = Vec::new();
        s.history(13, &mut h);
        assert_eq!(h.len(), FIELD_FRAMES);
        assert_eq!((h[0].fno, h[1].fno, h[2].fno, h[3].fno), (13, i64::MIN, 11, 10), "frame 12 never arrived and reads empty");
        assert!(h[0].p[0] > 0.0);
        // A frame RING later lands in the same slot: frame 13's history no longer shows it.
        s.push((13 + RING as i64) * FRAME, &tone);
        s.history(13, &mut h);
        assert_eq!(h[0].fno, i64::MIN);
    }
}
