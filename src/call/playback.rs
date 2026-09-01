//! Kept-recording playback (docs/calls.md) — decode a kept call, sum its channels to MONO, play through the speaker.
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
}

impl PlaybackHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for PlaybackHandle {
    fn drop(&mut self) {
        // Dropping the handle (a new playback replacing an old, or the app tearing down) is a stop edge — the worker sees it and releases the session.
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Play a KEPT recording blob through the speaker (downmixed to mono). `None` if a call is active, the blob is missing/unreadable, or there's no audio device.
pub fn play_blob(identity_seed: &[u8; 32], content_hash: &[u8; 32]) -> Option<PlaybackHandle> {
    if crate::platform::audio::is_active() {
        crate::log("CALL playback: audio busy (call active) — refused");
        return None;
    }
    let bytes = crate::storage::blob_load(identity_seed, content_hash)?;
    let stream = crate::call::record::open_blob(&bytes)?;
    spawn(stream)
}

/// Preview the LIVE spool through the same downmix, before the Keep/Delete decision has finalized a blob (the end-screen Play). Borrows the ticket — never consumes or shreds it.
pub fn play_spool(ticket: &SpoolTicket) -> Option<PlaybackHandle> {
    if crate::platform::audio::is_active() {
        crate::log("CALL playback: audio busy (call active) — refused");
        return None;
    }
    let records = crate::call::spool::drain_records(ticket)?;
    let stream = crate::call::record::stream_from_records(&records)?;
    spawn(stream)
}

fn spawn(stream: KeptStream) -> Option<PlaybackHandle> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    // We are the owner (checked !is_active() above): start flips ACTIVE false→true.
    if !crate::platform::audio::start() {
        crate::platform::audio::stop();
        return None;
    }
    let spawned = std::thread::Builder::new()
        .name("call-playback".into())
        .spawn(move || {
            run(stream, &flag);
            crate::platform::audio::stop(); // only we flipped ACTIVE true — safe to release
        })
        .is_ok();
    if !spawned {
        crate::platform::audio::stop();
        return None;
    }
    Some(PlaybackHandle { stop })
}

fn run(mut stream: KeptStream, stop: &AtomicBool) {
    let nchan = stream.nchan.max(1);
    while !stop.load(Ordering::Relaxed) {
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
    }
    // Let the DAC finish rendering the tail before the caller's stop() releases the session.
    while !stop.load(Ordering::Relaxed) && crate::platform::audio::playback_depth() > 0 {
        std::thread::sleep(Duration::from_millis(1));
    }
}
