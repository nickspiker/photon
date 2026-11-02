//! Kept-recording playback (docs/waves.md) — decode a kept wave, sum its channels to MONO, play thru the speaker.
//!
//! Mirrors the media engine's shape: a stop flag + a worker thread that OWNS the audio session for the life of the playback (`platform::audio::start()` on spawn, `stop()` on exit). Playback never touches the cpal streams directly (they are `!Send` and live on the audio thread) — it only pushes decoded mono frames to the global `PLAYBACK_Q` via `queue_playback`, exactly as the live engine's receive path does.
//!
//! **One owner.** A live wave owns the audio session; playback REFUSES to start while `platform::audio::is_active()`. Symmetrically, the wave-answer / offer paths stop any running playback before they start the engine (`self.playback.take().map(|p| p.stop())`). Only the owner that flipped `ACTIVE false→true` calls `stop()`.
//!
//! **No wall-clock timer.** The pacing clock is the DAC draining `PLAYBACK_Q` — one frame per 10 ms of real hardware time. The worker decodes the next frame only once the queue has drained below a small target, polling that depth on a 1 ms granularity (the same poll cadence the engine and ring loop use — a worker-thread poll, not a wave-state timer).

use crate::wave::record::KeptStream;
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
    /// Each channel's loudness in the frame just queued — mean square of its samples as f32 bits, one per channel — what the replayed wave screen lights each person's ring with (Nick 2026-11-01: "wave screen replayed").
    levels: Arc<Vec<std::sync::atomic::AtomicU32>>,
    /// Channels left out of the mix, one bit per channel (bit k = channel k): a person muted on the replay screen. A recording of more than 64 voices plays the rest unmutable.
    muted: Arc<std::sync::atomic::AtomicU64>,
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

    /// Channel `k`'s loudness in the frame just played (mean square, 0 = silence or no such channel).
    pub fn level(&self, k: usize) -> f32 {
        // WHY/PROOF: `k` comes from the replay screen's tiles, which may name more people than the recording has channels (a roster larger than the links that were up).
        self.levels.get(k).map_or(0.0, |l| f32::from_bits(l.load(Ordering::Relaxed)))
    }

    /// Leave channel `k` out of the mix, or bring it back.
    pub fn set_muted(&self, k: usize, muted: bool) {
        // WHY/PROOF: the mask is 64 bits; a shift by 64 or more is undefined for u64, so channels past 63 stay in the mix.
        if k < 64 {
            if muted {
                self.muted.fetch_or(1 << k, Ordering::Relaxed);
            } else {
                self.muted.fetch_and(!(1 << k), Ordering::Relaxed);
            }
        }
    }

    pub fn is_muted(&self, k: usize) -> bool {
        k < 64 && self.muted.load(Ordering::Relaxed) & (1 << k) != 0
    }
}

impl Drop for PlaybackHandle {
    fn drop(&mut self) {
        // Dropping the handle (a new playback replacing an old, or the app tearing down) is a stop edge — the worker sees it and releases the session.
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Play a KEPT recording blob thru the speaker (downmixed to mono). `None` if a wave is active, the blob is missing/unreadable, or there's no audio device.
pub fn play_blob(identity_seed: &[u8; 32], content_hash: &[u8; 32]) -> Option<PlaybackHandle> {
    play_blob_from(identity_seed, content_hash, 0)
}

/// Play a kept recording from archive slot `slot` (10 ms units) — the wave card's tap-to-seek. The stream seeks by walking packet lengths, so a far target costs a byte scan plus a few priming decodes.
pub fn play_blob_from(identity_seed: &[u8; 32], content_hash: &[u8; 32], slot: usize) -> Option<PlaybackHandle> {
    if crate::platform::audio::is_active() {
        crate::log("WAVE playback: audio busy (wave active) — refused");
        return None;
    }
    let bytes = crate::storage::blob_load(identity_seed, content_hash)?;
    let stream = crate::wave::record::open_blob(&bytes)?;
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
    let levels: Arc<Vec<std::sync::atomic::AtomicU32>> = Arc::new((0..nchan).map(|_| std::sync::atomic::AtomicU32::new(0)).collect());
    let muted = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (levels_w, muted_w) = (levels.clone(), muted.clone());
    // We are the owner (checked !is_active() above): start flips ACTIVE false→true.
    if !crate::platform::audio::start() {
        crate::platform::audio::stop();
        return None;
    }
    // Local source: preview frames must reach the DAC verbatim — the network-jitter splice/trims were the field's "choppy and distorted" playback (2026-09-08).
    crate::platform::audio::set_local_source(true);
    let spawned = std::thread::Builder::new()
        .name("wave-playback".into())
        .spawn(move || {
            run(stream, &flag, skip, &pos_w, &seek_w, &levels_w, &muted_w);
            done_flag.store(true, Ordering::SeqCst);
            crate::platform::audio::stop(); // only we flipped ACTIVE true — safe to release
        })
        .is_ok();
    if !spawned {
        crate::platform::audio::stop();
        return None;
    }
    Some(PlaybackHandle { stop, done, pos, seek, total, envelope, env_per_sec, nchan, levels, muted })
}

fn run(mut stream: KeptStream, stop: &AtomicBool, skip: usize, pos: &std::sync::atomic::AtomicUsize, seek: &std::sync::atomic::AtomicUsize, levels: &[std::sync::atomic::AtomicU32], muted: &std::sync::atomic::AtomicU64) {
    let nchan = stream.nchan.max(1); // WHY/PROOF: the stream's channel count is the container header's — file data; the frame stride divides by it
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
        // Each channel's loudness this frame (mean square), for the replay screen's rings.
        let frames = inter.len() / nchan;
        for (k, l) in levels.iter().enumerate().take(nchan) {
            let ms = inter.iter().skip(k).step_by(nchan).map(|&s| (s as f32) * (s as f32)).sum::<f32>() / frames.max(1) as f32; // WHY/PROOF: a frame with no samples (a zero-length decode) divides by one and reads silent, never NaN
            l.store(ms.to_bits(), Ordering::Relaxed);
        }
        // Downmix to mono: average the channels (÷nchan avoids the +6 dB sum overflow), leaving out every muted one.
        let mask = muted.load(Ordering::Relaxed);
        let mono: Vec<i16> = inter
            .chunks_exact(nchan)
            .map(|c| (c.iter().enumerate().filter(|(k, _)| *k >= 64 || mask & (1 << k) == 0).map(|(_, &s)| s as i32).sum::<i32>() / nchan as i32) as i16)
            .collect();
        crate::platform::audio::queue_playback(mono);
        pos.fetch_add(1, Ordering::Relaxed);
    }
    // Let the DAC finish rendering the tail before the origin's stop() releases the session.
    while !stop.load(Ordering::Relaxed) && crate::platform::audio::playback_depth() > 0 {
        std::thread::sleep(Duration::from_millis(1));
    }
}
