//! Ringback (Nick 2026-09-03): while WE wave someone, THEIR ring plays in OUR earpiece — the callee's identity cadence, so the caller hears who they're waving and has an unmistakable "the wave is going out" signal that no spinner can give.
//!
//! It rides the CALL output path (not the notification stream), padded by the same [`crate::call::OUTPUT_PAD_STOPS`] the live wave uses, so the ringback and the conversation that follows land at one consistent loudness in the ear.
//!
//! **The calibration bonus.** The ringback is a KNOWN signal played into the room while the near human is almost certainly NOT talking (nobody talks to a phone that hasn't connected yet) — which is exactly the far-talks-alone condition the in-call learner needs and usually has to wait a whole conversation to get. Opening the audio session for the ringback means the mic is capturing while it plays, so the same [`crate::call::learn::Learner`] that runs in-call runs here on the ring instead of the peer's voice. By the time the callee answers we can already hand the engine a measured (g, delay) for this route instead of a stale ritual seed — the exact failure convicted in field waves 1-2 (docs: the learner never armed inside a 41 s call, so both sides ducked on wrong seeds).
//!
//! Session ownership: [`start`] opens the audio session and the guard closes it on drop UNLESS the engine has taken over (answered — the engine's own `audio::start()` is a no-op against a live session, so the ringback→call transition never tears the device down and never clicks).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::learn::{Confidence, Learner};

/// What the ringback measured before the callee picked up — handed to the engine as its calibration seed.
#[derive(Debug, Clone, Copy)]
pub struct RingbackProbe {
    /// Coupling gain, volume-normalized, as the live learner reports it.
    pub g_norm: f32,
    /// Render→capture delay in 10ms bins.
    pub delay_bins: usize,
    pub confidence: Confidence,
    pub windows: usize,
    pub floor: f32,
}

/// The probe the most recent ringback produced, if it reached any pooled estimate. Read once at answer.
static PROBE: Mutex<Option<RingbackProbe>> = Mutex::new(None);

/// Take the last ringback's measurement (clears it — one call, one seed).
pub fn take_probe() -> Option<RingbackProbe> {
    PROBE.lock().unwrap().take()
}

/// Stops the ringback when dropped — every Outgoing teardown edge (answered, declined, hung up, glare-folded) is a stop edge for free, the [`super::RingGuard`] pattern.
pub struct RingbackGuard(Arc<AtomicBool>);

impl Drop for RingbackGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// 44.1kHz chirp → 48kHz call path, linear. The cadence is a fixed buffer built once per wave, so this runs off the audio thread and correctness beats sophistication (same trade as the platform layer's device resampler).
fn to_call_rate(src: &[f32], src_rate: u32) -> Vec<f32> {
    let dst_rate = crate::platform::audio::SAMPLE_RATE;
    if src_rate == dst_rate || src.is_empty() {
        return src.to_vec();
    }
    let ratio = src_rate as f64 / dst_rate as f64;
    let n_out = ((src.len() as f64) / ratio) as usize;
    let mut out = Vec::with_capacity(n_out);
    for i in 0..n_out {
        let pos = i as f64 * ratio;
        let idx = pos as usize;
        let frac = (pos - idx as f64) as f32;
        let a = src[idx.min(src.len() - 1)];
        let b = src[(idx + 1).min(src.len() - 1)];
        out.push(a + (b - a) * frac);
    }
    out
}

/// Start the ringback for a wave we are placing. `digest` is the relationship digest — the SAME seed as the callee's own ring, so what we hear is genuinely their cadence.
/// Returns `None` when the audio session won't open (a mic-less/speaker-less box still waves, just silently).
pub fn start(digest: [u8; 32]) -> Option<RingbackGuard> {
    if !crate::platform::audio::start() {
        crate::log("CALL: ringback — audio session refused, waving silently");
        return None;
    }
    *PROBE.lock().unwrap() = None;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let spawned = std::thread::Builder::new()
        .name("call-ringback".into())
        .spawn(move || run(digest, flag))
        .is_ok();
    if !spawned {
        crate::log("CALL: ringback thread spawn failed");
        crate::platform::audio::stop();
        return None;
    }
    Some(RingbackGuard(stop))
}

fn run(digest: [u8; 32], stop: Arc<AtomicBool>) {
    use crate::platform::audio;

    // One cadence, resampled to the call rate and padded to the headset level — the ring is a full-scale clip and the wave that follows is 4 stops down; landing them at the same loudness is the whole point of routing it thru the call path.
    let ring = chirp::Chirp::ring_from_hash(digest);
    let cadence = to_call_rate(ring.samples(), chirp::SAMPLE_RATE_HZ);
    let pad = 1.0 / (1u32 << super::OUTPUT_PAD_STOPS) as f32;
    let frames: Vec<Vec<i16>> = cadence
        .chunks(audio::FRAME_SAMPLES)
        .map(|c| {
            let mut f: Vec<i16> = c
                .iter()
                .map(|&s| (s * pad * i16::MAX as f32) as i16)
                .collect();
            f.resize(audio::FRAME_SAMPLES, 0);
            f
        })
        .collect();
    let gap_frames =
        (chirp::RING_REPEAT_GAP_SECS * audio::SAMPLE_RATE as f64 / audio::FRAME_SAMPLES as f64) as usize;

    // The probe: the same learner the engine runs, fed the ring as its far reference. bt_route widens its scan exactly as in-call; no stored seed — this measurement IS the seed.
    let route = audio::route_id();
    let mut learner = Learner::new(route.starts_with("bt:"), None, None);
    let mut env_cursor = 0usize;

    // Feed the queue a couple of frames ahead of the drain and sleep a frame at a time: the jitter buffer is built for a network source, so handing it the whole cadence at once would just make it trim.
    let mut queued: usize = 0;
    let mut i = 0usize;
    let total = frames.len() + gap_frames;
    while !stop.load(Ordering::Relaxed) {
        while audio::playback_depth() < 4 && !stop.load(Ordering::Relaxed) {
            let slot = i % total;
            audio::queue_playback(if slot < frames.len() {
                frames[slot].clone()
            } else {
                vec![0i16; audio::FRAME_SAMPLES] // the repeat gap
            });
            i += 1;
            queued += 1;
        }
        // Probe: drain the render envelope (what the DAC actually emitted, post-jitter) and the raw mic, exactly as the engine's learner tap does.
        let (env, cur) = audio::render_env_since(env_cursor);
        env_cursor = cur;
        for (osc, e) in env {
            learner.push_far(osc, e);
        }
        for frame in audio::captured_frames() {
            let mean = if frame.is_empty() {
                0.0
            } else {
                frame.iter().map(|s| s.unsigned_abs() as u64).sum::<u64>() as f32 / frame.len() as f32
            };
            learner.push_mic(vsf::eagle_time_oscillations(), mean);
        }
        // Volume-normalize exactly as the engine does (engine.rs's vol_lin_now): the probe's g is published as a UNIT-VOLUME figure and the predictive duck re-scales it by the live vol_lin. Ticking 1.0 here would hand the engine a g measured at whatever the media slider happened to be, which it would then scale AGAIN — under-ducking by the volume factor, the direction that inflicts echo on the peer. On Android the mirrored stream is STREAM_MUSIC, which IS the knob governing our USAGE_MEDIA render; desktop has no volume API and stays 1.0.
        let vol_lin = crate::platform::audio::current_volume_db().map_or(1.0, |db| 10f32.powf(db / 20.0));
        learner.tick(vol_lin);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // Publish whatever the ring taught us. Confidence is usually None over a short wave — the engine still takes the (g, delay) as a seed, which is strictly better than a stale ritual profile measured on another day.
    let est = learner.estimate();
    if let (Some(g), Some(d)) = (est.g_norm, est.delay_bins) {
        crate::logf!(
            "CALL: ringback probe — g {:.4} delay {}ms conf {:?} over {} window(s); floor {:.0}; {} cadence frame(s) played",
            g,
            d * 10,
            format!("{:?}", est.confidence),
            est.windows,
            est.floor,
            queued
        );
        *PROBE.lock().unwrap() = Some(RingbackProbe {
            g_norm: g,
            delay_bins: d,
            confidence: est.confidence,
            windows: est.windows,
            floor: est.floor,
        });
    } else {
        crate::logf!(
            "CALL: ringback probe — no estimate (rejects {:?}, floor {:.0}); the wave was too short to pool a window",
            format!("{:?}", est.rejects),
            est.floor
        );
    }

    // Hand the session over to the engine if the wave was answered; otherwise close it — an unanswered wave must not leave the mic open.
    if super::media_sink_live() {
        crate::log("CALL: ringback stopped — engine has the session");
    } else {
        crate::platform::audio::stop();
    }
}
