//! Kept-recording playback (docs/calls.md) — decode a kept call, sum its channels to MONO, play thru the speaker.
//!
//! Mirrors the media engine's shape: a stop flag + a worker thread that OWNS the audio session for the life of the playback (`platform::audio::start()` on spawn, `stop()` on exit). Playback never touches the cpal streams directly (they are `!Send` and live on the audio thread) — it only pushes decoded mono frames to the global `PLAYBACK_Q` via `queue_playback`, exactly as the live engine's receive path does.
//!
//! **One owner.** A live call owns the audio session; playback REFUSES to start while `platform::audio::is_active()`. Symmetrically, the call-answer / offer paths stop any running playback before they start the engine (`self.playback.take().map(|p| p.stop())`). Only the owner that flipped `ACTIVE false→true` calls `stop()`.
//!
//! **No wall-clock timer.** The pacing clock is the DAC draining `PLAYBACK_Q` — one frame per 10 ms of real hardware time. The worker decodes the next frame only once the queue has drained below a small target, polling that depth on a 1 ms granularity (the same poll cadence the engine and ring loop use — a worker-thread poll, not a call-state timer).

use crate::call::record::KeptStream;
use crate::call::spool::SpoolTicket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Keep the render queue this shallow so playback tracks the DAC rather than racing ahead — a few frames of jitter cushion, no more.
const PACE_TARGET: usize = 6;

pub struct PlaybackHandle {
    stop: Arc<AtomicBool>,
    /// Set by the worker on exit (end of recording or stop) — the UI polls it to flip the Play/Stop pill back without any timer.
    done: Arc<AtomicBool>,
    /// Frames queued so far (skip included) — the scrub bar's numerator.
    pos: Arc<std::sync::atomic::AtomicUsize>,
    /// A pending seek (archive slot), `usize::MAX` = none. The worker takes it between frames and repositions the SAME stream — a scrub never tears the audio session down (2026-09-10: restarting playback per seek raced the old worker's release and read "can't play now").
    seek: Arc<std::sync::atomic::AtomicUsize>,
    /// Total frames in the stream — the denominator.
    pub total: usize,
    /// The container's fine envelope (`nchan × env_len`, eighth-stops) — the card draws this instead of the row thumbnail while the handle lives.
    pub envelope: Vec<u8>,
    pub env_per_sec: u8,
    pub nchan: usize,
}

impl PlaybackHandle {
    /// Reposition the running playback to archive slot `slot` — taken by the worker between frames.
    pub fn seek(&self, slot: usize) {
        self.seek.store(slot.min(self.total), Ordering::SeqCst);
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn is_finished(&self) -> bool {
        self.done.load(Ordering::Relaxed)
    }

    pub fn position(&self) -> usize {
        self.pos.load(Ordering::Relaxed)
    }
}

impl Drop for PlaybackHandle {
    fn drop(&mut self) {
        // Dropping the handle (a new playback replacing an old, or the app tearing down) is a stop edge — the worker sees it and releases the session.
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Play a KEPT recording blob thru the speaker (downmixed to mono). `None` if a call is active, the blob is missing/unreadable, or there's no audio device.
pub fn play_blob(identity_seed: &[u8; 32], content_hash: &[u8; 32]) -> Option<PlaybackHandle> {
    play_blob_from(identity_seed, content_hash, 0)
}

/// Play a kept recording from archive slot `slot` (10 ms units) — the wave card's tap-to-seek. The stream seeks by walking packet lengths, so a far target costs a byte scan plus a few priming decodes.
pub fn play_blob_from(identity_seed: &[u8; 32], content_hash: &[u8; 32], slot: usize) -> Option<PlaybackHandle> {
    if crate::platform::audio::is_active() {
        crate::log("CALL playback: audio busy (call active) — refused");
        return None;
    }
    let bytes = crate::storage::blob_load(identity_seed, content_hash)?;
    let stream = crate::call::record::open_blob(&bytes)?;
    spawn(stream, slot)
}

fn spawn(stream: KeptStream, skip: usize) -> Option<PlaybackHandle> {
    let stop = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let pos = Arc::new(std::sync::atomic::AtomicUsize::new(skip.min(stream.total)));
    let seek = Arc::new(std::sync::atomic::AtomicUsize::new(usize::MAX));
    let seek_w = seek.clone();
    let total = stream.total;
    let envelope = stream.envelope.clone();
    let env_per_sec = stream.env_per_sec;
    let nchan = stream.nchan;
    let flag = stop.clone();
    let done_flag = done.clone();
    let pos_w = pos.clone();
    // We are the owner (checked !is_active() above): start flips ACTIVE false→true.
    if !crate::platform::audio::start() {
        crate::platform::audio::stop();
        return None;
    }
    // Local source: preview frames must reach the DAC verbatim — the network-jitter splice/trims were the field's "choppy and distorted" playback (2026-09-08).
    crate::platform::audio::set_local_source(true);
    let spawned = std::thread::Builder::new()
        .name("call-playback".into())
        .spawn(move || {
            run(stream, &flag, skip, &pos_w, &seek_w);
            done_flag.store(true, Ordering::SeqCst);
            crate::platform::audio::stop(); // only we flipped ACTIVE true — safe to release
        })
        .is_ok();
    if !spawned {
        crate::platform::audio::stop();
        return None;
    }
    Some(PlaybackHandle { stop, done, pos, seek, total, envelope, env_per_sec, nchan })
}

fn run(mut stream: KeptStream, stop: &AtomicBool, skip: usize, pos: &std::sync::atomic::AtomicUsize, seek: &std::sync::atomic::AtomicUsize) {
    let nchan = stream.nchan.max(1);
    // Seek = a length-prefix walk to just before the mark plus a few priming decodes (KeptStream::seek) — never a decode of everything before it.
    if skip > 0 {
        stream.seek(skip);
    }
    while !stop.load(Ordering::Relaxed) {
        // A scrub landed: reposition this stream in place. The few frames already queued at the DAC play out (~30ms), then audio continues from the mark.
        let want = seek.swap(usize::MAX, Ordering::SeqCst);
        if want != usize::MAX {
            stream.seek(want);
            pos.store(want, Ordering::Relaxed);
        }
        // Backpressure = the pacing clock: wait until the DAC has drained below the target, then decode+queue the next frame. The output callback pops one frame per 10 ms of hardware time; we poll depth on a 1 ms granularity, never sleeping to a wall time.
        while !stop.load(Ordering::Relaxed) && crate::platform::audio::playback_depth() >= PACE_TARGET {
            std::thread::sleep(Duration::from_millis(1));
        }
        let Some(inter) = stream.next_frame() else {
            break;
        };
        // Downmix to mono: average the channels (÷nchan avoids the +6 dB sum overflow).
        let mono: Vec<i16> = inter
            .chunks_exact(nchan)
            .map(|c| (c.iter().map(|&s| s as i32).sum::<i32>() / nchan as i32) as i16)
            .collect();
        crate::platform::audio::queue_playback(mono);
        pos.fetch_add(1, Ordering::Relaxed);
    }
    // Let the DAC finish rendering the tail before the caller's stop() releases the session.
    while !stop.load(Ordering::Relaxed) && crate::platform::audio::playback_depth() > 0 {
        std::thread::sleep(Duration::from_millis(1));
    }
}
