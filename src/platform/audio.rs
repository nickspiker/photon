//! Wave audio I/O — the ONE capture/playback surface for waves (docs/waves.md), platform-split under a shared queue core.
//!
//! The wave engine speaks 48kHz mono i16 in 5ms frames (240 samples — the 2026-09-08 latency flag day; was 10ms/480) and never touches a device API: it drains `captured_frames()` and feeds `queue_playback()`. Under that:
//! - **Desktop**: a dedicated audio thread owns the cpal input+output streams (cpal streams are !Send — built and parked on their own thread, torn down when `stop()` clears the active flag). Device-rate/channel conversion happens at the callback edge via a naive linear resampler — correctness first; a better resampler is a drop-in.
//! - **Android**: Kotlin owns AudioRecord/AudioTrack and crosses JNI into the same queues: `nativeAudioCaptured` pushes mic frames, `nativeAudioNextFrame` pulls render frames. Both ends ride the LOW-LATENCY paths (capture: VOICE_RECOGNITION raw fast-track; render: USAGE_MEDIA fast mixer) — vendor voice-pipeline processing is deliberately OFF both ways (Nick, latency-first 2026-08-20), so echo control belongs to OUR canceller over RENDER_REF. Start/stop ride the MESSAGE_NOTIFIER service ref like notifications do.
//!
//! **AEC plumbing from day one** (the retrofit-misery lesson): every rendered sample lands in an eagle-stamped reference ring BEFORE it reaches the device, whether or not any canceller exists yet. When a canceller (or the suppression duck) arrives, its far-end reference is already exact — the decoded signal we handed the DAC, not a guess at what some stack played.
//!
//! Queues are bounded drop-oldest: realtime audio must never block and never balloon — a stalled consumer costs the oldest 5ms, not memory or latency.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::Mutex;

/// The wave engine's sample rate — everything above the device edge is 48kHz mono.
pub const SAMPLE_RATE: u32 = 48_000;
/// 5ms @ 48kHz mono — the CELT frame the engine encodes (the 2026-09-08 flag day: halving the frame halves the fill wait, the window batch, AND the jitter quantum in one move).
pub const FRAME_SAMPLES: usize = 240;

/// Mic frames waiting for the engine (drop-oldest past ~500ms): (true time its FIRST sample was captured, that sample's position in the stream's 48 kHz sample count, samples). The time is the HAL's boot-clock timestamp on Android and the callback instant less the device's reported capture delay on desktop, both mapped thru TrueClock (docs/lock.md §5.1); the position is what the sender's rate regression fits against.
/// Captured mic frames at 24-BIT depth (i32 holding −2^23..2^23−1; a 16-bit source is shifted up 8 — Nick 2026-09-14: "are we not doing 24 bit voice capture?"). The engine's fixed makeup consumes the extra bits directly (acc >> 40 into the i16 wire), so a calibrated Unprocessed mic at 40 LSB16 arrives as 10240 LSB24 and the makeup lifts SIGNAL, not dither.
static CAPTURE_Q: Mutex<VecDeque<(i64, i64, Vec<i32>)>> = Mutex::new(VecDeque::new());
/// Decoded far-end frames waiting for the device (drop-oldest past ~1s).
static PLAYBACK_Q: Mutex<VecDeque<Vec<i16>>> = Mutex::new(VecDeque::new());
/// The AEC far-end reference: (eagle osc at enqueue-to-device, samples) per frame, last ~500ms. The canceller/duck reads this; nothing else does.
static RENDER_REF: Mutex<VecDeque<(i64, Vec<i16>)>> = Mutex::new(VecDeque::new());
static RENDER_REF_TOTAL: AtomicUsize = AtomicUsize::new(0);
/// Audio session live? Device loops run only while true.
static ACTIVE: AtomicBool = AtomicBool::new(false);

const CAPTURE_Q_MAX: usize = 50; // 500ms
const PLAYBACK_Q_MAX: usize = 240; // 1.2s of 5ms frames — must EXCEED the 200-frame v-chirp (drop-oldest at push beheaded the 1s chirp against the old 100, field 2026-09-08: the second beheading mechanism after the ceiling trims)
const RENDER_REF_MAX: usize = 100; // 500ms of 5ms frames

/// The in-wave learner's far-end reference: (eagle osc at DAC-enqueue, mean |sample| envelope) per rendered frame — the envelope-only sibling of RENDER_REF, deep enough (~10s) for the learner's sliding correlation windows without cloning 48KB frame snapshots per tick. Silence/priming frames land as env 0.0, which is the CORRECT reference (that is what actually hit the DAC). Post-jitter post-splice, so the render→capture delay measured against it is route-constant.
static RENDER_ENV: Mutex<VecDeque<(i64, f32)>> = Mutex::new(VecDeque::new());
const RENDER_ENV_MAX: usize = 2048; // ~10s of 5ms frames
/// Monotonic count of entries ever pushed — the cursor base for `render_env_since`.
static RENDER_ENV_TOTAL: AtomicUsize = AtomicUsize::new(0);

// NAMED PLAYOUT (docs/lock.md §7, Nick 2026-09-25: "if it's not there, don't play it; if it gets there late and we are still within the window, cue it up and start it late … no fade in either. honest and clean"). The far party's frames arrive NAMED by grid sample (the sender's microphone instant) and are played by name against true time: each render asks for the samples whose names belong at the DAC's true instant minus the playout latency L. A sample that is here plays; one that is not plays as zero; one whose instant has passed is never played; nothing is repeated, faded, spliced or primed.
// L (target) = the engine's 1-in-256 point of arrival ages plus the repair copy's offset (spec §7.1 as amended 2026-09-28); l (actual) follows it one sample at a time at the stream's zero-slope and sign-change points (named_frame). It absorbs a peer whose clock is off (spec §7.5) because the offset is part of every age.
const JITTER_FLOOR: usize = 1; // the loss loop's least margin: one 5 ms frame over the path floor
const JITTER_CAP: usize = 24; // 120 ms of margin — the most the engine's loop may ask for, even on a bad relay
static JITTER_TARGET: AtomicUsize = AtomicUsize::new(JITTER_FLOOR);

/// Engine hook: the loss-rate loop's margin over the path floor, in frames.
pub fn set_jitter_target(frames: usize) {
    // WHY/PROOF: the margin's physical range — under one frame every jitter spike is a miss, past the cap the latency is a phone call from the moon; the loss controller's output is held to it here, at the one store.
    JITTER_TARGET.store(frames.clamp(JITTER_FLOOR, JITTER_CAP), Ordering::Relaxed);
}

/// Named playout is live (a wave's far channel); off = the plain FIFO for local sources.
static NAMED: AtomicBool = AtomicBool::new(false);
/// The far party's decoded frames by grid name (k0 of the frame's first sample), ascending.
static WAVE_RX: Mutex<VecDeque<(i64, Vec<i16>)>> = Mutex::new(VecDeque::new());
/// L, the TARGET playout latency in samples (engine-set: the 1-in-256 point of arrival ages plus the repair copy's offset); `i64::MIN` until the engine sets it.
static PLAY_TARGET: AtomicI64 = AtomicI64::new(i64::MIN);
/// l, the ACTUAL playout latency in samples at the start of the last rendered frame (DAC true instant minus the name being played); `i64::MIN` before the first render.
static PLAY_L: AtomicI64 = AtomicI64::new(i64::MIN);
/// The read cursor: the next grid name the speaker plays. Names before it have had their instant (played, or slipped past) and are never played again.
static PLAY_NEXT: AtomicI64 = AtomicI64::new(i64::MIN);

/// The grid sample the speaker plays next, or None before named playout has begun — the wave screen's RX field ripples from here.
pub fn play_head() -> Option<i64> {
    let k = PLAY_NEXT.load(Ordering::Relaxed);
    (k != i64::MIN).then_some(k)
}
/// Some far audio has reached the speaker this wave — misses count only after it has.
static PLAYED_ANY: AtomicBool = AtomicBool::new(false);
/// Missing samples a frame may have before it counts as a miss: the DAC's own drift against true time opens a one-sample gap now and then, which is not the network losing anything.
const MISS_SLACK: usize = 2;

/// Wave playout on: the far channel plays by name from here until the session's queues clear.
pub fn set_named_playout(on: bool) {
    NAMED.store(on, Ordering::Relaxed);
}

/// Engine hook: L, the target playout latency in samples.
pub fn set_play_target(samples: i64) {
    PLAY_TARGET.store(samples, Ordering::Relaxed);
}

/// One decoded far frame, named by the grid sample of its first sample. A frame whose instant has already passed is dropped here — it would never play.
pub fn queue_named(k0: i64, frame: Vec<i16>) {
    if k0 + frame.len() as i64 <= PLAY_NEXT.load(Ordering::Relaxed) {
        LATE_DROPPED.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let mut q = WAVE_RX.lock().unwrap();
    let at = q.partition_point(|(k, _)| *k < k0);
    if q.get(at).is_some_and(|(k, _)| *k == k0) {
        return; // the same name twice (a fill racing the live copy) — the first one stands
    }
    q.insert(at, (k0, frame));
}

/// Build the far channel's render frame for the DAC instant `at_osc` (true time of its first sample).
/// l FOLLOWS L BY SLIPS (Nick 2026-09-28): each output sample reads the next name at the cursor; at a sample where the stream's slope is zero or changes sign, and l ≠ L, exactly one sample is dropped (l too long) or repeated (l too short). Nothing else ever moves l: no jumps, no waiting for silence, no cap — a silent (all-zero) stretch is eligible at every sample, so l meets L there at once, and speech offers a slip at every peak and trough.
/// Missing names read as zero (a gap plays as silence, and silence is where l moves freest). A name whose instant has passed is never played.
/// The DAC's own crystal drift lands in l the same way (it is the DAC's true instant that sets l), so the §7.3 drift loop is this same corrector.
/// Output latency, callback to DAC, in grid samples on OUR clock (offset-free): summed per named frame for the wave's latency budget.
static OUT_AHEAD_SUM: AtomicI64 = AtomicI64::new(0);
static OUT_AHEAD_N: AtomicI64 = AtomicI64::new(0);
/// The output lead of the latest named frame, samples (callback → DAC); i64::MIN before any. L adds it (engine.rs).
static OUT_AHEAD_LAST: AtomicI64 = AtomicI64::new(i64::MIN);

/// The output lead now: how long before the DAC a render frame must be ready — L has to cover it or every frame arrives after its callback (field 2026-09-29: Emma's voice path runs 220 ms callback → DAC; L without it dropped 8957 of ~9000 frames as too late).
pub fn output_ahead() -> Option<i64> {
    let v = OUT_AHEAD_LAST.load(Ordering::Relaxed);
    (v != i64::MIN).then_some(v)
}

/// Mean output latency (samples) since the last read, then restarts the mean; `None` before any named frame.
pub fn take_output_ahead_mean() -> Option<i64> {
    let n = OUT_AHEAD_N.swap(0, Ordering::Relaxed);
    let s = OUT_AHEAD_SUM.swap(0, Ordering::Relaxed);
    (n > 0).then(|| s / n)
}

fn named_frame(at_osc: i64) -> Vec<i16> {
    let ahead = vsf::grid::eagle_to_sample(at_osc) - vsf::grid::eagle_to_sample(crate::network::time_base::now_osc());
    OUT_AHEAD_LAST.store(ahead.max(0), Ordering::Relaxed);
    OUT_AHEAD_SUM.fetch_add(ahead, Ordering::Relaxed);
    OUT_AHEAD_N.fetch_add(1, Ordering::Relaxed);
    let mut out = vec![0i16; FRAME_SAMPLES];
    let target = PLAY_TARGET.load(Ordering::Relaxed);
    if target == i64::MIN {
        return out; // no target yet — nothing can be due
    }
    let dac_k = vsf::grid::eagle_to_sample(at_osc);
    let mut p = PLAY_NEXT.load(Ordering::Relaxed);
    // The first frame starts exactly on L. After that nothing but slips ever moves the cursor — however far L moves, l walks to it (Nick 2026-09-28: no hardcoded timings, no jumps).
    if p == i64::MIN {
        p = dac_k - target;
    }
    NAMED_LAST_P.store(p, Ordering::Relaxed);
    // The names this frame can reach: the one before the cursor (for the slope), and at most two per output sample (a drop per sample in a silent stretch).
    let lo = p - 1;
    let span = 2 * FRAME_SAMPLES + 3;
    let mut win = vec![0i16; span];
    let mut have = vec![false; span];
    {
        let q = WAVE_RX.lock().unwrap();
        for (k0, f) in q.iter() {
            if *k0 >= lo + span as i64 {
                break;
            }
            let a = (*k0).max(lo);
            let b = (*k0 + f.len() as i64).min(lo + span as i64);
            for k in a..b {
                win[(k - lo) as usize] = f[(k - *k0) as usize]; // PROOF: lo ≤ a ≤ k < b ≤ lo + span, and k0 ≤ k < k0 + len
                have[(k - lo) as usize] = true;
            }
        }
    }
    let at = |k: i64| -> i32 { win[(k - lo) as usize] as i32 }; // PROOF: every k read below lies in [p−1, p + 2·FRAME + 1] ⊂ [lo, lo + span)
    let mut missing = 0usize;
    // ONE slip per point: a repeat leaves the cursor on the same sample, and that sample must not qualify again on the next output (it would chain repeats at one peak).
    let mut slipped_at = i64::MIN;
    for (i, o) in out.iter_mut().enumerate() {
        let l = dac_k + i as i64 - p;
        let (d0, d1) = (at(p) - at(p - 1), at(p + 1) - at(p));
        let eligible = p != slipped_at && (d0 == 0 || d1 == 0 || (d0 > 0) != (d1 > 0));
        if eligible && l > target {
            p += 1; // DROP: this sample is skipped, l shortens by one
        }
        if !have[(p - lo) as usize] {
            missing += 1;
        }
        *o = at(p) as i16;
        if eligible && l < target {
            slipped_at = p; // a REPEAT leaves the cursor where it is, lengthening l by one
        } else {
            p += 1;
        }
    }
    PLAY_L.store(dac_k + FRAME_SAMPLES as i64 - p, Ordering::Relaxed); // the lag the next sample will play at
    PLAY_NEXT.store(p, Ordering::Relaxed);
    // Every name before the cursor has had its instant.
    {
        let mut q = WAVE_RX.lock().unwrap();
        while q.front().is_some_and(|(k0, f)| *k0 + f.len() as i64 <= p) {
            q.pop_front();
        }
    }
    let got = FRAME_SAMPLES - missing;
    NAMED_LAST_MISSING.store(missing, Ordering::Relaxed);
    if PLAYED_ANY.load(Ordering::Relaxed) && missing > MISS_SLACK {
        JITTER_UNDERRUNS.fetch_add(1, Ordering::Relaxed);
    }
    if got > 0 {
        PLAYED_ANY.store(true, Ordering::Relaxed);
    }
    out
}
/// The earpiece trim in stops (−6..=6), device-local; see the render chain.
static RX_TRIM_STOPS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
/// Six stops (Lun) each way (Nick 2026-09-16): a bar with a tick per stop shows how close the ear sits to loud-max and quiet-min.
pub const RX_TRIM_MAX_STOPS: i32 = 6;

pub fn rx_trim_stops() -> i32 {
    RX_TRIM_STOPS.load(Ordering::Relaxed)
}

pub fn set_rx_trim_stops(stops: i32) {
    // WHY/PROOF: the trim is a HUMAN's input (the Wave page's steppers, a synced setting) — held to the ± range the shift math below is sized for.
    RX_TRIM_STOPS.store(stops.clamp(-RX_TRIM_MAX_STOPS, RX_TRIM_MAX_STOPS), Ordering::Relaxed);
}
// Telemetry: named-playout misses (a frame with samples due that never arrived in time), reset per wave in clear_queues, logged at session teardown.
static JITTER_UNDERRUNS: AtomicUsize = AtomicUsize::new(0);
/// Far frames that arrived after their instant had passed — never played (the honest "frames shed").
static LATE_DROPPED: AtomicUsize = AtomicUsize::new(0);

/// Per-wave playout diagnostics: (the loss loop's margin frames — telemetry only now, far frames waiting, misses, frames too late to play).
pub fn jitter_stats() -> (usize, usize, usize, usize) {
    (JITTER_TARGET.load(Ordering::Relaxed), WAVE_RX.lock().unwrap().len(), JITTER_UNDERRUNS.load(Ordering::Relaxed), LATE_DROPPED.load(Ordering::Relaxed))
}

/// l, the actual playout latency in samples; `None` before the first far frame.
pub fn play_latency() -> Option<i64> {
    let l = PLAY_L.load(Ordering::Relaxed);
    (l != i64::MIN).then_some(l)
}

/// L, the target playout latency in samples; `None` before the engine sets it.
pub fn play_target() -> Option<i64> {
    let l = PLAY_TARGET.load(Ordering::Relaxed);
    (l != i64::MIN).then_some(l)
}

/// Peak-held mean |sample| of what the device is rendering (~80ms decay) — the engine's soft duck reads this as the far-end activity signal, covering the device-buffer + acoustic lag without sample-accurate alignment.
static FAR_LEVEL: AtomicUsize = AtomicUsize::new(0);
// THE SPEAKER DUCK (Nick 2026-09-13: "if the mic level gets hot, drop the speaker volume right before it gets played, do not keep record of post filtered values"). The mic goes to the wire untouched; the only echo control is here, on the render frame, in the moment: gain = 1 − near / SPEAKER_DUCK_MIC_FULL clamped to [0, 1], where `near` is the mean |sample| of the newest captured mic frame (the engine notes it per frame). No slew, no floor, no hold, and nothing downstream records the scaled frame — the learner's envelope tap and the canceller reference read what was emitted, which is the point of them.
/// The duck gain in Q32, PRE-COMPUTED on the capture side (`note_near_level` does the subtract-and-shift once per captured frame) so the render pull carries zero arithmetic beyond load + kernel. Unity outside a wave and while the mic is muted.
static SPEAKER_DUCK_GAIN: AtomicI64 = AtomicI64::new(crate::wave::qgain::UNITY);
/// The duck kernel's carried remainder (see wave/qgain.rs) — only the render thread touches it.
static SPEAKER_DUCK_CARRY: AtomicI64 = AtomicI64::new(0);
/// THE COUPLING-AWARE DUCK LAW (Nick 2026-09-13 night: the duck should scale with what the speaker actually leaks into the mic — rocker down = less echo = less duck; and "we scale our values so we don't lose any data or have any hard discontinuities"). gain = UNITY − near·k·2^7, where `near` is the plan-unit mic mean and `k` (Q16) is the MEASURED echo-per-emitted: during frames where the far side is rendering and the near mic sits under the emitted level, the mic is mostly echo, so k ≈ near/emitted — an EMA (>>6 per qualifying frame, τ ≈ a third of a second of far-talk-alone). Passive, no chirp, no volume mirror (the mirror lies on exclusive-MMAP routes — Nick's reads −32 dB while sounding proper); a rocker change re-converges k over ~a second of their speech, CONTINUOUSLY — the gain never steps, and the render kernel's carried remainder stays exact across every gain change. At k = K_REF (1/16, the earpiece midpoint) the law is exactly the old FULL=8192 map: plan-level talking halves the speaker, a shout silences it.
/// k's neutral seed in Q16 (1/16), and its clamp: 1/256 (a clean earpiece barely ducks) to 1/2 (a hot loudspeaker ducks hard).
pub const DUCK_K_REF_Q16: i64 = 1 << 12;
const DUCK_K_MIN_Q16: i64 = 1 << 8;
const DUCK_K_MAX_Q16: i64 = 1 << 15;
static DUCK_K_Q16: AtomicI64 = AtomicI64::new(DUCK_K_REF_Q16);
/// The receive loss plan's echo bound, Q16: k·speaker_gain is held at or under this. 1/64 ≈ −36 dB (0.05/−26 dB until 2026-09-14: "still a bit echo-ey and loud" on a wave with k 0.03 — at our ~100 ms acoustic round trip the ear wants echo under −40 dB, the same figure the POTS hybrid standards settled on; the loss this costs is loudness at the ceiling, which was the other complaint).
const RX_ECHO_MARGIN_Q16: i64 = 1024;
/// The downward expander's knee in plan units (voiced speech ≈ 2048; 256 is −18 dB under it): frames below taper linearly toward silence.
const RX_EXPAND_KNEE: i64 = 256;
/// Mean |sample| of the newest frame handed to the DAC (post-duck — what the room actually receives), the k estimator's denominator. Reuses the FAR_LEVEL sum.
static EMITTED_LEVEL: AtomicUsize = AtomicUsize::new(0);
/// THE FOLLOWERS (Nick 2026-10-02, the fishtank: "we're dealing in 5 ms timescales… we should be averaging more"): a 5 ms mean of |sample| on a 100-150 Hz voice spans under one pitch period, so it ripples at the pitch rate and every gain fed by it modulated the far voice at that rate.
/// Each detector now runs a partial add — Nick's `new = (7·old + target) >> 3` on the way down (≈40 ms at 5 ms frames), `(old + target) >> 1` on the way up (≈10 ms, so a duck still catches an onset) — sitting across two or more pitch periods.
/// The followed near level (plan units) that the duck gain reads; the k learner keeps the raw frame (a minimum statistic wants the measurement, not its smoothing).
static NEAR_ENV: AtomicI64 = AtomicI64::new(0);
/// The followed far level (the frame about to render) that the expander reads.
static FAR_ENV: AtomicI64 = AtomicI64::new(0);
/// THE RAMP (same day): a gain computed once per frame used to multiply all 240 samples as one constant — a staircase with a step every 5 ms, each step a discontinuity. The render kernel now walks from the gain the previous frame ENDED on to this frame's target, one Q32 increment per sample; this is where it ended.
static RENDER_GAIN_LAST: AtomicI64 = AtomicI64::new(crate::wave::qgain::UNITY);

/// THE RENDER TRACE (Nick 2026-10-02, "Go!" on the instrument): what the DAC actually got, frame by frame, with the name it played, the composed gain it was scaled by and how many of its samples were holes.
/// The engine drains this ring each loop iteration into the spool's render channel (spool.rs RENDER_CHAN) and the 100 ms trace line; nothing is pushed while profiling is off.
pub struct RenderTrace {
    /// The grid name the frame started playing (the playout cursor at its first sample).
    pub name: i64,
    /// The composed render gain (duck · loss · expander) this frame was walked toward, Q32.
    pub gain_q32: i64,
    /// Samples of this frame that had no far audio to play (holes).
    pub missing: u16,
    /// The frame as handed to the DAC, after the gains, the trim and the overlay.
    pub pcm: Vec<i16>,
}
static PROFILE_ON: AtomicBool = AtomicBool::new(false);
static PROFILE_RING: Mutex<VecDeque<RenderTrace>> = Mutex::new(VecDeque::new());
/// Two seconds of 5 ms frames: the engine drains every loop pass; a ring this deep only ever matters if the engine stalls, and then the newest frames are the ones worth having.
const PROFILE_RING_MAX: usize = 400;
/// The name and the hole count of the last named frame — named_frame leaves them for the render pull to trace.
static NAMED_LAST_P: AtomicI64 = AtomicI64::new(i64::MIN);
static NAMED_LAST_MISSING: AtomicUsize = AtomicUsize::new(0);
/// Playout re-anchors this wave (see `reanchor_playout`).
static REANCHORS: AtomicUsize = AtomicUsize::new(0);

/// Arm or disarm the render trace.
pub fn set_profile(on: bool) {
    PROFILE_ON.store(on, Ordering::Relaxed);
    if !on {
        PROFILE_RING.lock().unwrap().clear();
    }
}

/// Take every traced render frame since the last take, oldest first.
pub fn take_render_trace() -> Vec<RenderTrace> {
    PROFILE_RING.lock().unwrap().drain(..).collect()
}

/// The followed near and far levels, plan units — the trace line prints them.
pub fn envelopes() -> (i64, i64) {
    (NEAR_ENV.load(Ordering::Relaxed), FAR_ENV.load(Ordering::Relaxed))
}

/// A CLOCK STEP RE-ANCHOR (Nick 2026-10-02, the Emma wave that played at 2x): the far party's names can only run ahead of the cursor when the sender's clock stepped against ours, and the doctrine already answers that — the first arrival sets L, whatever it is.
/// Forgetting the cursor makes the next render the first frame again: it starts exactly on the L the engine just restarted from the stepped arrival. One jump, by the first-arrival rule; ordinary jitter stays a walk.
pub fn reanchor_playout() {
    PLAY_NEXT.store(i64::MIN, Ordering::Relaxed);
    REANCHORS.fetch_add(1, Ordering::Relaxed);
}

/// Playout re-anchors since the last audio reset.
pub fn reanchors() -> usize {
    REANCHORS.load(Ordering::Relaxed)
}

/// One step of the follower: fast on the way up, Nick's seven-eighths on the way down.
pub fn follow(env: i64, target: i64) -> i64 {
    if target > env {
        (env + target) >> 1
    } else {
        (7 * env + target) >> 3
    }
}

/// The speaker duck's tally since the last audio reset: render frames pulled, frames at or under half gain (the mic was hot), and the summed gain in 1/1024 (mean gain = sum / frames) — the engine's echo line and teardown readout. A frames-touched count was useless (the room floor alone puts every frame a hair under 1).
static SPEAKER_FRAMES: AtomicUsize = AtomicUsize::new(0);
static SPEAKER_HALF: AtomicUsize = AtomicUsize::new(0);
static SPEAKER_GAIN_SUM: AtomicUsize = AtomicUsize::new(0);
/// The engine arms the speaker duck for routes with an acoustic path (earpiece, loudspeaker, unknown) and disarms it for a headset, at engine start and on a mid-wave route swap.
static SPEAKER_DUCK_ARMED: AtomicBool = AtomicBool::new(false);

/// What the far end is acoustically coupled to — the echo-layer dispatcher. `Headset` = no acoustic path, bypass everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioRoute {
    Headset,
    Builtin,
    /// Loudspeaker — the speaker-toggle target. Falls into the "ducks" bucket like Builtin (only `Headset` bypasses the echo layers).
    Speaker,
    /// Earpiece (phone receiver) — the speaker-toggle's off state on a handset.
    Earpiece,
    Unknown,
}

/// Requested output-route override (the speaker toggle). `None` = follow the sniffed device route. v1 is a pure INTENT seam: desktop records the state + logs (real device switching is a later layer); Android-future flips `AudioManager.setSpeakerphoneOn` from here.
static ROUTE_OVERRIDE: Mutex<Option<AudioRoute>> = Mutex::new(None);

/// Speaker-toggle intent. Stores the requested route (observable via [`route_override`]); no real device switch in v1.
pub fn set_route(r: AudioRoute) {
    *ROUTE_OVERRIDE.lock().unwrap() = Some(r);
    let name = match r {
        AudioRoute::Headset => "headset",
        AudioRoute::Builtin => "builtin",
        AudioRoute::Speaker => "speaker",
        AudioRoute::Earpiece => "earpiece",
        AudioRoute::Unknown => "unknown",
    };
    crate::logf!("AUDIO: route intent = {} (stub — no device switch yet)", name);
}

/// The current route override, if the speaker toggle set one.
pub fn route_override() -> Option<AudioRoute> {
    *ROUTE_OVERRIDE.lock().unwrap()
}

// ---------------------------------------------------------------------------
// Route IDENTITY + device volume — the calibration substrate (docs plan 2026-09-02).
// A calibration profile keys on WHICH output path is live (`speaker` vs `bt:<name>` couple very differently), and the echo prediction scales with the device volume the profile was measured at. Desktop fills these from the cpal device sniff (no portable volume API → volume stays None, the in-wave tracker absorbs drift); Android mirrors both down from Kotlin (AudioDeviceCallback + volume broadcast), the nativeImeInset pattern.
// ---------------------------------------------------------------------------

/// Normalized identity of the current OUTPUT path — the calibration-profile key. Examples: `speaker`, `earpiece`, `headset`, `bt:AirPods Pro`, `unknown:ALSA default`. Empty until the first device sniff/mirror.
static ROUTE_ID: Mutex<String> = Mutex::new(String::new());

/// Current output volume in dB (Android `getStreamVolumeDb`; `None` on desktop / pre-mirror).
static VOLUME_DB: Mutex<Option<f32>> = Mutex::new(None);

/// Identity of the current INPUT (mic) — the voice-profile key (Nick 2026-09-02: the voice measurement is a property of the MIC + user, not the output route; splitting the keys means switching speaker→earpiece keeps your voice profile). `builtin-mic` / `bt:<name>` / `mic:<device>`; empty until mirrored.
static MIC_ID: Mutex<String> = Mutex::new(String::new());

/// Kotlin's mic introspection at wave-audio start (Android): whether the vendor DECLARES the CDD Unprocessed calibration, the chosen input's 94 dB SPL sensitivity in dBFS (NaN = vendor reports unknown), and the input inventory string. The makeup's second-priority source; the log's ground truth for "is this raw feed calibrated or lawless".
static MIC_INFO: Mutex<Option<(bool, f32, String)>> = Mutex::new(None);

pub fn set_mic_info(unprocessed_declared: bool, sensitivity_dbfs: f32, desc: String) {
    crate::logf!(
        "AUDIO: mic introspection — unprocessed calibration {}, sensitivity {} dBFS @ 94 dB SPL, inputs [{}]",
        if unprocessed_declared { "DECLARED" } else { "NOT declared (the raw feed's gain is vendor-lawless)" },
        if sensitivity_dbfs.is_finite() { format!("{sensitivity_dbfs:.1}") } else { "unknown".into() },
        desc
    );
    *MIC_INFO.lock().unwrap() = Some((unprocessed_declared, sensitivity_dbfs, desc));
}

/// The chosen input's sensitivity (dBFS at 94 dB SPL), when the vendor reports one.
pub fn mic_sensitivity_dbfs() -> Option<f32> {
    MIC_INFO.lock().unwrap().as_ref().map(|(_, s, _)| *s).filter(|s| s.is_finite())
}

pub fn mic_id() -> String {
    MIC_ID.lock().unwrap().clone()
}

pub(crate) fn set_mic_identity(id: String) {
    *MIC_ID.lock().unwrap() = id;
}

pub fn route_id() -> String {
    ROUTE_ID.lock().unwrap().clone()
}

/// The mean absolute level of one 16-bit frame — the one level every duck, envelope and stall guard reads.
/// WHY the empty case: a caller may hand an empty frame (nothing captured yet), and an empty frame has no level.
/// PROOF: zero is that level; a bare `sum / len` would divide by zero, so the one place that divides says so here instead of `.max(1)` at every caller.
pub fn mean_abs(frame: &[i16]) -> u64 {
    if frame.is_empty() {
        return 0;
    }
    frame.iter().map(|s| s.unsigned_abs() as u64).sum::<u64>() / frame.len() as u64
}

/// The same level for a 24-bit capture frame (held in i32), in 24-bit units. Empty reads as silence, as above.
pub fn mean_abs_24(frame: &[i32]) -> i64 {
    if frame.is_empty() {
        return 0;
    }
    frame.iter().map(|s| s.unsigned_abs() as i64).sum::<i64>() / frame.len() as i64
}

pub fn current_volume_db() -> Option<f32> {
    *VOLUME_DB.lock().unwrap()
}

/// Install the route identity (platform layer only: the cpal output sniff or the Kotlin device callback).
pub(crate) fn set_route_identity(id: String) {
    *ROUTE_ID.lock().unwrap() = id;
}

#[cfg(target_os = "android")]
pub(crate) fn set_volume_db(db: Option<f32>) {
    *VOLUME_DB.lock().unwrap() = db;
}

/// Drain every captured frame since the last call (5ms 48kHz mono each). Engine-side, any thread.
pub fn captured_frames() -> Vec<(i64, i64, Vec<i32>)> {
    let mut q = CAPTURE_Q.lock().unwrap();
    q.drain(..).collect()
}

/// Queue one decoded frame for render. Engine-side, any thread.
pub fn queue_playback(frame: Vec<i16>) {
    let mut q = PLAYBACK_Q.lock().unwrap();
    if q.len() >= PLAYBACK_Q_MAX {
        q.pop_front();
    }
    q.push_back(frame);
}

/// Frames currently queued for render — the engine's jitter-buffer depth signal.
pub fn playback_depth() -> usize {
    PLAYBACK_Q.lock().unwrap().len()
}

/// Snapshot the far-end reference ring (for the suppression duck / a future canceller).
pub fn render_reference() -> Vec<(i64, Vec<i16>)> {
    RENDER_REF.lock().unwrap().iter().cloned().collect()
}

/// The duck's far-end activity signal: peak-held mean |sample| of current render, decaying ~80ms.
pub fn far_level() -> u32 {
    FAR_LEVEL.load(Ordering::Relaxed) as u32
}

/// Arm (routes with an acoustic path) or disarm (headset) the speaker duck. Arming re-seeds k at the reference — a route swap is new physics, the estimator starts from neutral and re-learns.
pub fn set_speaker_duck(armed: bool) {
    SPEAKER_DUCK_ARMED.store(armed, Ordering::Relaxed);
    DUCK_K_Q16.store(DUCK_K_REF_Q16, Ordering::Relaxed);
}

/// The duck law: the FIXED presence slope `UNITY − near·2^20` (full duck at plan-unit mic 4096 = twice the plan level, as 8192 was to the old 4096 plan — the field-passed 0.95.21 shape). k is measured and PRINTED but deliberately out of the gain path (2026-09-13 23:30: the 35× mic makeup sits INSIDE the echo loop, so true plan-unit coupling on a normal earpiece is ~0.3-1.0 — feeding measured k in as the slope silenced the far voice at any k ≈ 0.4; the coupling-aware law needs the margin form, gain ≤ ε·near/(k·far), designed against k telemetry across rocker positions, not another guessed slope).
pub fn duck_gain_q32(near: i64, _k_q16: i64) -> i64 {
    // WHY/PROOF: a duck only ever TAKES gain away — a loud near end drives UNITY − near past zero, and a negative gain would invert the speaker; the clip to [0, UNITY] is the duck's definition.
    (crate::wave::qgain::UNITY - (near << 20)).clamp(0, crate::wave::qgain::UNITY)
}

/// The engine notes the plan-unit mean |sample| of each captured mic frame here; the k estimator and the Q32 duck gain both run HERE (capture cadence) so the render pull only loads. Muted zeros keep k untouched and the gain at unity.
pub fn note_near_level(mean: u32) {
    let near = mean as i64;
    let emitted = EMITTED_LEVEL.load(Ordering::Relaxed) as i64;
    // k is a MINIMUM statistic, not a mean (field 2026-09-13 23:00, two waves: the EMA version could not tell echo from Emma's loud room — room/emitted ≈ 0.3 fed k as if it were coupling, and deeper duck → smaller emitted → bigger ratio was a RUNAWAY to k 0.37-0.46 that silenced the far voice outright). True echo scales WITH the emitted level, so near/emitted ≈ k in every echo-only instant, ducked or not; room and voice sit ON TOP, so every sample is ≥ k and the min over far-active frames converges to the truth in the quiet gaps. Down instantly; up by k>>9 per far-active frame (τ ≈ 2.5 s of far speech) so a rocker-up re-learns without ever stepping.
    if SPEAKER_DUCK_ARMED.load(Ordering::Relaxed) && emitted > 256 {
        // WHY/PROOF: one frame's coupling ratio is a noisy MEASUREMENT (near-end level over what we emitted); the learner accepts it only inside the physically plausible band, so a transient cannot teach the duck a coupling no speaker has.
        let sample = ((near << 16) / emitted).clamp(DUCK_K_MIN_Q16, DUCK_K_MAX_Q16);
        let k = DUCK_K_Q16.load(Ordering::Relaxed);
        let next = if sample < k { sample } else { (k + (k >> 9)).min(DUCK_K_MAX_Q16) };
        DUCK_K_Q16.store(next, Ordering::Relaxed);
    }
    // The duck keys on the mic level ABOVE plausible echo (field 2026-09-14 00:15, the echo-ey wave: at 65-76x makeup the far side's own echo inflated `near` while they talked, so each talker was ducked BY their echo — chop with zero loss). k is the min-statistic lower bound, doubled for margin; the subtraction is continuous, no gate: echo-only mic → input ~0 → full duplex; real speech → input ≈ the voice.
    let k = DUCK_K_Q16.load(Ordering::Relaxed);
    let echo_est = (2 * k * emitted) >> 16;
    // The duck keys on the FOLLOWED near level (see NEAR_ENV): the pitch-rate ripple of a 5 ms mean no longer reaches the gain.
    let env = follow(NEAR_ENV.load(Ordering::Relaxed), near);
    NEAR_ENV.store(env, Ordering::Relaxed);
    let g = duck_gain_q32((env - echo_est).max(0), k); // the algorithm: the echo estimate can exceed the near level, and a negative residual is simply no near talker
    SPEAKER_DUCK_GAIN.store(g, Ordering::Relaxed);
}

/// Mean |sample| of the newest frame handed to the DAC — the engine gates the voiced-calibration accumulator on far-quiet with it.
pub fn emitted_level() -> i64 {
    EMITTED_LEVEL.load(Ordering::Relaxed) as i64
}

/// The duck's live coupling estimate (Q16) — the echo line prints it beside the tallies.
pub fn duck_k_q16() -> i64 {
    DUCK_K_Q16.load(Ordering::Relaxed)
}

/// `(render frames pulled, frames at or under half gain, mean gain over them as a per-mille)` since the last audio reset.
pub fn speaker_duck_stats() -> (u64, u64, u64) {
    let frames = SPEAKER_FRAMES.load(Ordering::Relaxed) as u64;
    let sum = SPEAKER_GAIN_SUM.load(Ordering::Relaxed) as u64;
    (
        frames,
        SPEAKER_HALF.load(Ordering::Relaxed) as u64,
        if frames == 0 { 1000 } else { sum * 1000 / (frames * 1024) },
    )
}

/// Render frames produced since process start — the stall watchdog's evidence that the speaker is being called back.
pub fn render_frames_total() -> usize {
    RENDER_REF_TOTAL.load(Ordering::Relaxed)
}

/// Rebuild the device streams in place (the stall watchdog). Android reopens both AAudio streams; elsewhere the device callback cannot silently die this way, so nothing.
pub fn rebuild_streams() {
    #[cfg(target_os = "android")]
    crate::platform::audio_aaudio::rebuild();
}

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// SESSION OWNERSHIP (field 2026-09-15 15:45, "Emma could not hear me"): every `start_owned` claims the session with a fresh generation; `stop_owned(gen)` tears it down ONLY while that generation still owns it. The ringback's late teardown (its thread finishes the probe estimate after the answer) used to race the engine's start on the UI thread — the engine's fresh input open met the ringback's still-closing input ("start: Disconnected"), then the ringback's stopWaveAudio pulled the foreground mic type and the earpiece route out from under the live wave: 0 packets out for the whole wave. A stale owner's stop is now a logged no-op.
static SESSION_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn start_owned() -> Option<u64> {
    if !start() {
        return None;
    }
    Some(SESSION_GEN.fetch_add(1, Ordering::SeqCst) + 1)
}

pub fn stop_owned(gen: u64) {
    let live = SESSION_GEN.load(Ordering::SeqCst);
    if live == gen {
        stop();
    } else {
        crate::logf!("AUDIO: stale owner's stop ignored (gen {} — the session belongs to gen {} now)", gen, live);
    }
}

pub(crate) fn push_captured(at_osc: i64, pos: i64, frame: Vec<i32>) {
    let mut q = CAPTURE_Q.lock().unwrap();
    if q.len() >= CAPTURE_Q_MAX {
        q.pop_front();
    }
    q.push_back((at_osc, pos, frame));
}

/// LOCAL-SOURCE playback (v-chirp probe, ringback, ended-screen preview): the queue is fed by a paced LOCAL source, not the network, so the whole adaptive apparatus must stand down — no priming, no target growth on dry (the source ENDING is dry), no standing-depth trims, no clock splice (there is no second clock to null). Field 2026-09-08, the conviction that unified three bugs: the engine dumps the 200-frame chirp into the queue at once and the hard ceiling TRIMMED IT FROM THE FRONT down to ~9 frames — the up-leg's low-frequency head never left the speaker (both field rejects: down-leg strong at a plausible lag, up-leg weak and late); the same splice/trim path made the ended-screen preview choppy and pitch-warped, and rode the ringback too.
static LOCAL_SOURCE: AtomicBool = AtomicBool::new(false);

pub fn set_local_source(on: bool) {
    LOCAL_SOURCE.store(on, Ordering::Relaxed);
}

/// THE USER'S OUTPUT (Nick 2026-09-29, "keeps the user chosen device, same with gain"): the route this device's user last picked on the route pill (a route id: "earpiece", "speaker", "bt:<name>", "headset:<name>"), persisted device-local as `audio.route`.
static PICKED_ROUTE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
/// The volume the user last set per route + usage (`<route>.v` voice / `<route>.m` media → stream index), persisted device-local as `audio.vol.<key>`.
static ROUTE_VOLUMES: std::sync::Mutex<Option<std::collections::HashMap<String, i32>>> = std::sync::Mutex::new(None);
/// Picks and volumes reported from the platform, waiting for the UI thread to persist them.
static ROUTE_PICK_OUT: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
static ROUTE_VOLUME_OUT: std::sync::Mutex<Vec<(String, i32)>> = std::sync::Mutex::new(Vec::new());
/// This device's voice-communication path was refused the fast path at the last open (the route pill says "slower" beside the earpiece).
static VOICE_PATH_SLOW: AtomicBool = AtomicBool::new(false);

/// The route + usage key a volume is kept under.
pub fn route_volume_key(route: &str, voice: bool) -> String {
    format!("{route}.{}", if voice { "v" } else { "m" })
}

/// Install the persisted pick (settings load).
pub fn set_picked_route(r: Option<String>) {
    *PICKED_ROUTE.lock().unwrap() = r;
}

pub fn picked_route() -> Option<String> {
    PICKED_ROUTE.lock().unwrap().clone()
}

/// Install the persisted per-route volumes (settings load).
pub fn set_route_volumes(m: std::collections::HashMap<String, i32>) {
    *ROUTE_VOLUMES.lock().unwrap() = Some(m);
}

pub fn route_volume(route: &str, voice: bool) -> Option<i32> {
    ROUTE_VOLUMES.lock().unwrap().as_ref()?.get(&route_volume_key(route, voice)).copied()
}

/// The platform saw the user pick `route`: remember it now, queue it for the UI to persist.
pub fn note_route_pick(route: String) {
    *PICKED_ROUTE.lock().unwrap() = Some(route.clone());
    *ROUTE_PICK_OUT.lock().unwrap() = Some(route);
}

/// The platform saw the user set the wave's volume on `route` + usage: remember it now, queue it for the UI to persist.
pub fn note_route_volume(route: &str, voice: bool, index: i32) {
    let key = route_volume_key(route, voice);
    ROUTE_VOLUMES.lock().unwrap().get_or_insert_with(Default::default).insert(key.clone(), index);
    ROUTE_VOLUME_OUT.lock().unwrap().push((key, index));
}

/// The UI thread's drain: a pick and the volume reports waiting to be persisted.
pub fn take_route_reports() -> (Option<String>, Vec<(String, i32)>) {
    (ROUTE_PICK_OUT.lock().unwrap().take(), std::mem::take(&mut *ROUTE_VOLUME_OUT.lock().unwrap()))
}

pub(crate) fn set_voice_path_slow(slow: bool) {
    VOICE_PATH_SLOW.store(slow, Ordering::Relaxed);
}

pub fn voice_path_slow() -> bool {
    VOICE_PATH_SLOW.load(Ordering::Relaxed)
}

/// THE OVERLAY (the connect / disconnect sweep, wave/sweep.rs): samples MIXED into whatever is playing, right before the DAC — never replacing the wave, never held back by its playout, never ducked. Popped one sample per rendered sample.
static OVERLAY: std::sync::Mutex<std::collections::VecDeque<i16>> = std::sync::Mutex::new(std::collections::VecDeque::new());
/// A fresh overlay's first sample has not rendered yet: the next render stamps its DAC instant into OVERLAY_START.
static OVERLAY_FRESH: AtomicBool = AtomicBool::new(false);
/// The DAC instant (Eagle oscillations) of the latest overlay's first sample; i64::MIN = not rendered yet.
static OVERLAY_START: AtomicI64 = AtomicI64::new(i64::MIN);
/// Signalled when the overlay runs dry — the edge the end sweep's deferred stop waits on.
static OVERLAY_DRAINED: (std::sync::Mutex<()>, std::sync::Condvar) = (std::sync::Mutex::new(()), std::sync::Condvar::new());
/// While the disconnect sweep plays, `stop()` is held: the session closes when the sweep has left the speaker.
static END_HOLD: AtomicBool = AtomicBool::new(false);

/// Mix `samples` over the output from the next rendered frame on.
pub fn play_overlay(samples: &[i16]) {
    let mut q = OVERLAY.lock().unwrap();
    q.clear();
    q.extend(samples.iter().copied());
    OVERLAY_START.store(i64::MIN, Ordering::Relaxed);
    OVERLAY_FRESH.store(true, Ordering::Relaxed);
}

/// The DAC instant of the current overlay's first sample, once it has rendered.
pub fn overlay_start_osc() -> Option<i64> {
    let s = OVERLAY_START.load(Ordering::Relaxed);
    (s != i64::MIN).then_some(s)
}

fn mix_overlay(frame: &mut [i16], at_osc: i64) {
    let mut q = OVERLAY.lock().unwrap();
    if q.is_empty() {
        return;
    }
    if OVERLAY_FRESH.swap(false, Ordering::Relaxed) {
        OVERLAY_START.store(at_osc, Ordering::Relaxed);
    }
    for s in frame.iter_mut() {
        match q.pop_front() {
            Some(o) => *s = s.saturating_add(o),
            None => break,
        }
    }
    if q.is_empty() {
        drop(q);
        let _g = OVERLAY_DRAINED.0.lock().unwrap();
        OVERLAY_DRAINED.1.notify_all();
    }
}

/// The disconnect sound (Nick 2026-09-29): a connected wave that ends plays the connect sweep reversed, and the audio session closes when it has left the speaker — `stop()` is held until the overlay drains, then this session's own stop runs (a wave that started in the meantime owns a new generation, and its session is left alone).
pub fn play_end_sweep() {
    if !is_active() {
        return;
    }
    let gen = SESSION_GEN.load(Ordering::SeqCst);
    play_overlay(&crate::wave::sweep::down());
    END_HOLD.store(true, Ordering::SeqCst);
    let _ = std::thread::Builder::new().name("end-sweep".into()).spawn(move || {
        let g = OVERLAY_DRAINED.0.lock().unwrap();
        // WHY/PROOF: the drain edge comes from the render callback; a device that stops calling back (unplugged, route torn down) can never drain the sweep, and the mic must not stay open waiting for it — twice the sweep's length bounds the hold.
        let _ = OVERLAY_DRAINED.1.wait_timeout_while(g, std::time::Duration::from_secs(2), |_| !OVERLAY.lock().unwrap().is_empty());
        END_HOLD.store(false, Ordering::SeqCst);
        stop_owned(gen);
    });
}

/// `stop()` is held while the disconnect sweep plays (see [`play_end_sweep`]).
fn end_sweep_holds_stop() -> bool {
    END_HOLD.load(Ordering::SeqCst)
}

/// Render the next frame at NOW — the tests' render call (every device path stamps its own DAC instant).
#[cfg(test)]
fn next_render_frame() -> Vec<i16> {
    next_render_frame_at(crate::network::time_base::eagle_at_boot_rt(crate::network::time_base::boot_now()))
}

/// The next render frame for the DAC instant `at_osc` (true time of its first sample, from the HAL on Android and the callback's playback delay on desktop), logged into the reference ring and envelope tap under that stamp.
/// A wave's far channel plays BY NAME (see NAMED PLAYOUT); local sources (the chirp, ringback, previews, kept-wave replay) play their queue in order, exactly once, silence when dry — neither path ever repeats, fades, splices or primes.
pub(crate) fn next_render_frame_at(at_osc: i64) -> Vec<i16> {
    let frame = if NAMED.load(Ordering::Relaxed) && !LOCAL_SOURCE.load(Ordering::Relaxed) {
        named_frame(at_osc)
    } else {
        PLAYBACK_Q.lock().unwrap().pop_front().unwrap_or_else(|| vec![0i16; FRAME_SAMPLES])
    };
    // The speaker duck: the live render frame scaled by the mic level of the moment, right before the DAC — Q32 with the carried remainder (wave/qgain.rs), the gain pre-shifted at capture time, so this path is load + mul-add-shift-and per sample. The chirp (a local source) plays at full — the probe measures the route, it must not be ducked by the voice that reads it.
    let mut frame = frame;
    let mut traced_gain = crate::wave::qgain::UNITY;
    if !LOCAL_SOURCE.load(Ordering::Relaxed) && SPEAKER_DUCK_ARMED.load(Ordering::Relaxed) {
        SPEAKER_FRAMES.fetch_add(1, Ordering::Relaxed);
        let duck = SPEAKER_DUCK_GAIN.load(Ordering::Relaxed);
        SPEAKER_GAIN_SUM.fetch_add((duck >> 22) as usize, Ordering::Relaxed);
        if duck <= crate::wave::qgain::UNITY / 2 {
            SPEAKER_HALF.fetch_add(1, Ordering::Relaxed);
        }
        // THE RECEIVE LOSS PLAN (2026-09-14, the echo-ey waves at 40-70× makeup: k 0.2 in plan units = a fifth of the earpiece back on the wire, the far talker hearing themselves at −14 dB). POTS bounded echo with static loss per link; ours is `min(1, RX_ECHO_MARGIN / k)` — the speaker is held where k·gain ≤ margin, so echo returns at −26 dB at worst whatever the rocker does (rocker up raises k, which lowers this). Loudness becomes physics-bounded, which is honest.
        let k = DUCK_K_Q16.load(Ordering::Relaxed).max(1); // WHY/PROOF: k is a LEARNED coupling that divides below; a learner that has seen nothing reads 0
        let loss = ((RX_ECHO_MARGIN_Q16 << 32) / k).min(crate::wave::qgain::UNITY);
        // THE DOWNWARD EXPANDER (Nick: "keep the ambient no talking level from screaming"): a continuous linear taper below a knee — the far room's floor and returning echo residue sink, speech above the knee passes at unity. Speaker-side, temporary, never recorded; no gate, no hold.
        // The expander keys on the FOLLOWED far level (see FAR_ENV): a quiet syllable tail no longer modulates itself at its own pitch rate.
        let level = follow(FAR_ENV.load(Ordering::Relaxed), mean_abs(&frame) as i64); // a mean of absolute values — never negative
        FAR_ENV.store(level, Ordering::Relaxed);
        let expand = ((level << 32) / RX_EXPAND_KNEE).min(crate::wave::qgain::UNITY);
        let g = crate::wave::qgain::compose(crate::wave::qgain::compose(duck, loss), expand);
        traced_gain = g;
        // THE RAMP: from the gain the previous frame ended on to this frame's target, one Q32 step per sample (see RENDER_GAIN_LAST). The step's integer division leaves the last sample within 240/2^32 of the target; the next frame starts from where this one actually ended, so nothing accumulates.
        let last = RENDER_GAIN_LAST.load(Ordering::Relaxed);
        if g != crate::wave::qgain::UNITY || last != crate::wave::qgain::UNITY {
            // Inline kernel (single render thread owns the carry): acc = s·g + carry; out = acc >> 32; carry = the low mask, exactly the residue. No saturation arm — g ≤ unity here, the product can only shrink.
            let step = (g - last) / FRAME_SAMPLES as i64;
            let mut gi = last;
            let mut carry = SPEAKER_DUCK_CARRY.load(Ordering::Relaxed);
            for s in frame.iter_mut() {
                gi += step;
                let acc = *s as i64 * gi + carry;
                *s = (acc >> 32) as i16;
                carry = acc & 0xFFFF_FFFF;
            }
            SPEAKER_DUCK_CARRY.store(carry, Ordering::Relaxed);
            RENDER_GAIN_LAST.store(gi, Ordering::Relaxed);
        }
    }
    // THE EARPIECE TRIM (Nick 2026-09-16, the Kalispell↔Southworth wave: a Pixel 3a at max rocker heard the plan level as quiet while a Pixel 8 Pro at its lowest step heard it loud — the earpieces differ by more than Android's narrow STREAM_VOICE_CALL rocker can span, and no vendor number tells us an earpiece's loudness). A per-device static gain in STOPS (one stop = ×2), remembered in `audio.rx.trim`, the pot under the rocker: a power of two, so it is an exact shift with no carry; upward it saturates at the rail. Never on the wire, never in the archive — this device's ear only. Local sources (ringback, previews) skip it: they were built for the rung the wave plays at.
    if !LOCAL_SOURCE.load(Ordering::Relaxed) {
        let stops = RX_TRIM_STOPS.load(Ordering::Relaxed);
        if stops > 0 {
            for s in frame.iter_mut() {
                // WHY/PROOF: a positive trim shifts an i16 up in i32, past ±32767, and an int→int `as` WRAPS — the clamp is the saturating gain, pinning overs to the rail instead of flipping their sign.
                *s = ((*s as i32) << stops).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            }
        } else if stops < 0 {
            for s in frame.iter_mut() {
                *s = (*s as i32 >> (-stops)) as i16;
            }
        }
    }
    // The connect / disconnect sweep rides OVER whatever plays (wave/sweep.rs), after the duck and the trim — it is ours, not the far side's.
    mix_overlay(&mut frame, at_osc);
    // The render trace: the frame exactly as the DAC gets it (see RenderTrace); only a named far frame is traced, and only while profiling.
    // A frame rendered before the first far frame was due has no name yet: the cursor is unset (i64::MIN), and tracing it under that name wrapped into a garbage stamp that wrecked the answering phone's keep (2026-10-02, Jeff's one-stream keep). Nothing to trace it under; nothing traced.
    if PROFILE_ON.load(Ordering::Relaxed) && NAMED.load(Ordering::Relaxed) && !LOCAL_SOURCE.load(Ordering::Relaxed) && NAMED_LAST_P.load(Ordering::Relaxed) != i64::MIN {
        let mut r = PROFILE_RING.lock().unwrap();
        if r.len() >= PROFILE_RING_MAX {
            r.pop_front();
        }
        r.push_back(RenderTrace {
            name: NAMED_LAST_P.load(Ordering::Relaxed),
            gain_q32: traced_gain,
            missing: NAMED_LAST_MISSING.load(Ordering::Relaxed).min(u16::MAX as usize) as u16,
            pcm: frame.clone(),
        });
    }
    // Far-end level for the duck: peak-hold with a per-frame decay (~80ms fall from full at 5ms frames — old/8 per frame; was old/4 at 10ms), so the mic stays attenuated across the device-buffer + acoustic lag rather than only the exact rendered instant.
    let lvl = mean_abs(&frame) as usize;
    let old = FAR_LEVEL.load(Ordering::Relaxed);
    FAR_LEVEL.store(lvl.max(old - old / 8), Ordering::Relaxed);
    // The k estimator's denominator: this mean is POST-duck — what the room actually receives — so k = near/emitted stays consistent whatever the duck is doing (both sides of the ratio scale together).
    EMITTED_LEVEL.store(lvl, Ordering::Relaxed);
    {
        let mut r = RENDER_REF.lock().unwrap();
        if r.len() >= RENDER_REF_MAX {
            r.pop_front();
        }
        r.push_back((at_osc, frame.clone()));
        RENDER_REF_TOTAL.fetch_add(1, Ordering::Relaxed);
    }
    // The learner's envelope tap — reuses `lvl` computed above (zero new arithmetic).
    {
        let mut r = RENDER_ENV.lock().unwrap();
        if r.len() >= RENDER_ENV_MAX {
            r.pop_front();
        }
        r.push_back((at_osc, lvl as f32));
        RENDER_ENV_TOTAL.fetch_add(1, Ordering::Relaxed);
    }
    frame
}

/// Drain the render-REFERENCE frames newer than `cursor` — the NLMS canceller's far-end sample feed (same cursor law as render_env_since). Frames that aged past the ring before a poll are gone; the consumer's resident-window check skips those stretches.
pub fn render_ref_since(cursor: usize) -> (Vec<(i64, Vec<i16>)>, usize) {
    let r = RENDER_REF.lock().unwrap();
    let total = RENDER_REF_TOTAL.load(Ordering::Relaxed);
    let missed = total - cursor; // the total only grows, and every cursor is a total this function returned
    let take = missed.min(r.len()); // the algorithm: the ring keeps only its newest entries — anything older than that fell off and is simply gone
    let out: Vec<(i64, Vec<i16>)> = r.iter().skip(r.len() - take).cloned().collect();
    (out, total)
}

/// Drain the render-envelope entries newer than `cursor` (a count of entries ever pushed; start at 0). Returns (new entries oldest-first, next cursor). The single learner consumer polls this each engine iteration; entries that aged past the ring before a poll are simply gone (the learner's window logic tolerates gaps — a stalled consumer loses history, never correctness).
pub fn render_env_since(cursor: usize) -> (Vec<(i64, f32)>, usize) {
    let r = RENDER_ENV.lock().unwrap();
    let total = RENDER_ENV_TOTAL.load(Ordering::Relaxed);
    let missed = total - cursor; // the total only grows, and every cursor is a total this function returned
    let take = missed.min(r.len()); // the algorithm: the ring keeps only its newest entries — anything older than that fell off and is simply gone
    let out: Vec<(i64, f32)> = r.iter().skip(r.len() - take).cloned().collect();
    (out, total)
}

/// Reset all queues — session start/stop hygiene so a new wave never hears the last wave's tail.
/// Logs the session's jitter diagnostics FIRST (this runs at both start and stop; the stop edge is the one whose numbers matter, and a start against zeroed stats logs nothing).
fn clear_queues() {
    let (margin, waiting, misses, late) = jitter_stats();
    if misses > 0 || waiting > 0 || late > 0 {
        let _ = margin;
        crate::logf!("WAVE: playout — l {} samples against L {}, {} far frame(s) left waiting, {} miss(es), {} too late to play (5 ms frames)", play_latency().map_or("?".to_string(), |l| l.to_string()), play_target().map_or("?".to_string(), |l| l.to_string()), waiting, misses, late);
    }
    CAPTURE_Q.lock().unwrap().clear();
    PLAYBACK_Q.lock().unwrap().clear();
    OVERLAY.lock().unwrap().clear();
    WAVE_RX.lock().unwrap().clear();
    NAMED.store(false, Ordering::Relaxed);
    PLAY_TARGET.store(i64::MIN, Ordering::Relaxed);
    PLAY_L.store(i64::MIN, Ordering::Relaxed);
    PLAY_NEXT.store(i64::MIN, Ordering::Relaxed);
    PLAYED_ANY.store(false, Ordering::Relaxed);
    RENDER_REF.lock().unwrap().clear();
    // RENDER_ENV clears but its TOTAL cursor base does NOT reset — a learner holding a cursor across the hygiene edge just sees a gap, never a phantom replay.
    RENDER_ENV.lock().unwrap().clear();
    // Each wave starts fresh at the least margin — never inheriting the last wave's grown latency.
    JITTER_TARGET.store(JITTER_FLOOR, Ordering::Relaxed);
    LOCAL_SOURCE.store(false, Ordering::Relaxed);
    JITTER_UNDERRUNS.store(0, Ordering::Relaxed);
    LATE_DROPPED.store(0, Ordering::Relaxed);
    OUT_AHEAD_LAST.store(i64::MIN, Ordering::Relaxed);
    FAR_LEVEL.store(0, Ordering::Relaxed);
    SPEAKER_DUCK_GAIN.store(crate::wave::qgain::UNITY, Ordering::Relaxed);
    SPEAKER_DUCK_CARRY.store(0, Ordering::Relaxed);
    NEAR_ENV.store(0, Ordering::Relaxed);
    FAR_ENV.store(0, Ordering::Relaxed);
    RENDER_GAIN_LAST.store(crate::wave::qgain::UNITY, Ordering::Relaxed);
    NAMED_LAST_P.store(i64::MIN, Ordering::Relaxed);
    NAMED_LAST_MISSING.store(0, Ordering::Relaxed);
    REANCHORS.store(0, Ordering::Relaxed);
    PROFILE_RING.lock().unwrap().clear();
    DUCK_K_Q16.store(DUCK_K_REF_Q16, Ordering::Relaxed);
    EMITTED_LEVEL.store(0, Ordering::Relaxed);
    SPEAKER_DUCK_ARMED.store(false, Ordering::Relaxed);
    SPEAKER_FRAMES.store(0, Ordering::Relaxed);
    SPEAKER_HALF.store(0, Ordering::Relaxed);
    SPEAKER_GAIN_SUM.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Desktop: cpal capture + playback on a dedicated audio thread.
// ---------------------------------------------------------------------------

#[cfg(not(any(target_os = "android", target_os = "redox")))]
mod desktop {
    use super::*;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::Sample as _;

    /// Naive linear resampler + channel folder: device-rate interleaved f32 → 48kHz mono i16 (and the reverse). Stateful across callbacks (fractional position carries over). Correctness-first; swap-in point for a windowed-sinc later.
    struct Resampler {
        ratio: f64, // src_rate / dst_rate
        pos: f64,
        prev: f32,
    }

    impl Resampler {
        fn new(src_rate: u32, dst_rate: u32) -> Self {
            Self {
                ratio: src_rate as f64 / dst_rate as f64,
                pos: 0.0,
                prev: 0.0,
            }
        }

        /// Consume mono f32 at src rate, emit mono f32 at dst rate.
        fn run(&mut self, input: &[f32], out: &mut Vec<f32>) {
            if (self.ratio - 1.0).abs() < 1e-9 {
                out.extend_from_slice(input);
                return;
            }
            // Virtual timeline: sample dst points across [prev, input...] by linear interpolation.
            let mut idx = self.pos;
            while (idx as usize) < input.len() {
                let i = idx as usize;
                let frac = idx - i as f64;
                let a = if i == 0 { self.prev } else { input[i - 1] };
                let b = input[i];
                out.push(a + (b - a) * frac as f32);
                idx += self.ratio;
            }
            self.pos = idx - input.len() as f64;
            self.prev = *input.last().unwrap_or(&self.prev);
        }
    }

    fn fold_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
        if channels <= 1 {
            return interleaved.to_vec();
        }
        interleaved
            .chunks_exact(channels)
            .map(|c| c.iter().sum::<f32>() / channels as f32)
            .collect()
    }

    /// Crude route sniff from the output device name — Headset means the echo layers all bypass. Real route change detection (device-change events) is a later layer; Unknown is the safe default (duck engages).
    fn sniff_route(name: &str) -> AudioRoute {
        let n = name.to_ascii_lowercase();
        if n.contains("headset")
            || n.contains("headphone")
            || n.contains("earbud")
            || n.contains("airpod")
        {
            AudioRoute::Headset
        } else {
            AudioRoute::Unknown
        }
    }

    static ROUTE: Mutex<AudioRoute> = Mutex::new(AudioRoute::Unknown);

    pub fn route() -> AudioRoute {
        *ROUTE.lock().unwrap()
    }

    /// Start the audio session: spawns the thread that owns both cpal streams for the life of the wave. Returns false when no devices exist (the wave proceeds one-way rather than failing — a mic-less desktop can still listen).
    pub fn start() -> bool {
        if ACTIVE.swap(true, Ordering::SeqCst) {
            return true; // already live
        }
        clear_queues();
        std::thread::Builder::new()
            .name("wave-audio".into())
            .spawn(audio_thread)
            .is_ok()
    }

    pub fn stop() {
        if super::end_sweep_holds_stop() {
            return; // the disconnect sweep is playing; its own stop follows the drain
        }
        ACTIVE.store(false, Ordering::SeqCst);
    }

    /// Build the capture stream for whatever sample format the device speaks (ALSA defaults love i16; everything folds thru f32 internally).
    fn build_capture<T>(
        dev: &cpal::Device,
        cfg: cpal::StreamConfig,
        channels: usize,
        src_rate: u32,
    ) -> Option<cpal::Stream>
    where
        T: cpal::SizedSample,
        f32: cpal::FromSample<T>,
    {
        let mut rs = Resampler::new(src_rate, SAMPLE_RATE);
        let mut pending: Vec<f32> = Vec::with_capacity(FRAME_SAMPLES * 2);
        // The float-cast carry (see the map below) — lives in the closure, one per stream.
        let mut cast_carry: f64 = 0.0;
        // The 48 kHz position of `pending[0]`.
        let mut framed_pos: i64 = 0;
        let stream = dev
            .build_input_stream(
                &cfg,
                move |data: &[T], info: &cpal::InputCallbackInfo| {
                    // When this buffer's first sample was CAPTURED: now, less the delay the device reports between capture and this callback — never "when the callback ran" (docs/lock.md §5.1).
                    let ts = info.timestamp();
                    let held = ts.callback.duration_since(&ts.capture).map_or(0, |d| crate::network::time_base::boot_ns_to_osc(d.as_nanos() as i64));
                    let cap_boot = crate::network::time_base::boot_now() - held;
                    let as_f32: Vec<f32> = data.iter().map(|s| s.to_sample::<f32>()).collect();
                    let mono = fold_mono(&as_f32, channels);
                    let mut out = Vec::with_capacity(mono.len());
                    rs.run(&mono, &mut out);
                    // (position, true time) of this callback's first sample — each frame's stamp extrapolates from it at the nominal rate (the engine's regression fits the real one).
                    let anchor = (framed_pos + pending.len() as i64, crate::network::time_base::eagle_at_boot_rt(cap_boot));
                    pending.extend(out);
                    while pending.len() >= FRAME_SAMPLES {
                        let frame: Vec<i32> = pending
                            .drain(..FRAME_SAMPLES)
                            .map(|s| {
                                // The one float boundary (the OS hands f32): error-feedback the cast at 24-bit — floor with the fraction carried to the next sample, zero-mean instead of a truncation bias.
                                // WHY/PROOF: as on Android — the 24-bit domain's rails, which the later i32 cast (±2^31) would not enforce, applied where the OS's float enters.
                                let acc = (s as f64 * 8_388_608.0).clamp(-8_388_608.0, 8_388_607.0) + cast_carry;
                                let out = acc.floor();
                                cast_carry = acc - out;
                                out as i32
                            })
                            .collect();
                        let at = anchor.1 + ((framed_pos - anchor.0) as i128 * crate::OSC_PER_SEC as i128 / SAMPLE_RATE as i128) as i64;
                        push_captured(at, framed_pos, frame);
                        framed_pos += FRAME_SAMPLES as i64;
                    }
                },
                |e| crate::logf!("AUDIO: capture stream error: {}", e),
                None,
            )
            .ok()?;
        stream.play().ok()?;
        Some(stream)
    }

    /// Build the render stream, format-generic like capture.
    fn build_render<T>(
        dev: &cpal::Device,
        cfg: cpal::StreamConfig,
        channels: usize,
        dst_rate: u32,
    ) -> Option<cpal::Stream>
    where
        T: cpal::SizedSample + cpal::FromSample<f32>,
    {
        let mut rs = Resampler::new(SAMPLE_RATE, dst_rate);
        let mut staged: VecDeque<f32> = VecDeque::new();
        let stream = dev
            .build_output_stream(
                &cfg,
                move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                    let ch = channels.max(1); // WHY/PROOF: the device's reported channel count — the buffer is chunked by it, and a chunk size of 0 panics
                    let need_mono = data.len() / ch;
                    // When this buffer's first sample reaches the DAC: now, plus the delay the device reports between this callback and playback (docs/lock.md §7.2) — named playout schedules against it.
                    let ts = info.timestamp();
                    let ahead = ts.playback.duration_since(&ts.callback).map_or(0, |d| crate::network::time_base::boot_ns_to_osc(d.as_nanos() as i64));
                    let dac_boot = crate::network::time_base::boot_now() + ahead;
                    while staged.len() < need_mono {
                        // Each pulled frame starts after everything already staged, at the device rate.
                        let offset = staged.len() as i128 * crate::OSC_PER_SEC as i128 / dst_rate as i128;
                        let frame = next_render_frame_at(crate::network::time_base::eagle_at_boot_rt(dac_boot + offset as i64));
                        let f: Vec<f32> = frame.iter().map(|&s| s as f32 / 32768.0).collect();
                        let mut out = Vec::with_capacity(f.len() + 8);
                        rs.run(&f, &mut out);
                        staged.extend(out);
                    }
                    for slot in data.chunks_exact_mut(ch) {
                        let s = staged.pop_front().unwrap_or(0.0);
                        for c in slot.iter_mut() {
                            *c = s.to_sample::<T>();
                        }
                    }
                },
                |e| crate::logf!("AUDIO: render stream error: {}", e),
                None,
            )
            .ok()?;
        stream.play().ok()?;
        Some(stream)
    }

    fn audio_thread() {
        use cpal::SampleFormat;
        let host = cpal::default_host();

        // ---- input (mic) ----
        let in_stream = host.default_input_device().and_then(|dev| {
            let name = dev.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "?".into());
            super::set_mic_identity(format!("mic:{name}"));
            let sc = dev.default_input_config().ok()?;
            let src_rate = sc.sample_rate();
            let channels = sc.channels() as usize;
            let fmt = sc.sample_format();
            crate::logf!(
                "AUDIO: capture on '{}' @ {}Hz x{}ch {} → 48k mono",
                dev.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "?".into()),
                src_rate,
                channels,
                format!("{:?}", fmt)
            );
            let cfg: cpal::StreamConfig = sc.into();
            match fmt {
                SampleFormat::F32 => build_capture::<f32>(&dev, cfg, channels, src_rate),
                SampleFormat::I16 => build_capture::<i16>(&dev, cfg, channels, src_rate),
                SampleFormat::U16 => build_capture::<u16>(&dev, cfg, channels, src_rate),
                other => {
                    crate::logf!("AUDIO: unsupported capture format {:?}", other);
                    None
                }
            }
        });
        if in_stream.is_none() {
            crate::log("AUDIO: no capture device — the wave is listen-only on this end");
        }

        // ---- output (speaker) ----
        let out_stream = host.default_output_device().and_then(|dev| {
            let name = dev.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "?".into());
            let sniffed = sniff_route(&name);
            *ROUTE.lock().unwrap() = sniffed;
            // Calibration-profile key: coarse route + the device name (a USB headset and the built-in jack must not share a profile).
            super::set_route_identity(match sniffed {
                AudioRoute::Headset => format!("headset:{name}"),
                _ => format!("out:{name}"),
            });
            let sc = dev.default_output_config().ok()?;
            let dst_rate = sc.sample_rate();
            let channels = sc.channels() as usize;
            let fmt = sc.sample_format();
            crate::logf!(
                "AUDIO: render on '{}' @ {}Hz x{}ch {} ← 48k mono (route {})",
                name,
                dst_rate,
                channels,
                format!("{:?}", fmt),
                format!("{:?}", route())
            );
            let cfg: cpal::StreamConfig = sc.into();
            match fmt {
                SampleFormat::F32 => build_render::<f32>(&dev, cfg, channels, dst_rate),
                SampleFormat::I16 => build_render::<i16>(&dev, cfg, channels, dst_rate),
                SampleFormat::U16 => build_render::<u16>(&dev, cfg, channels, dst_rate),
                other => {
                    crate::logf!("AUDIO: unsupported render format {:?}", other);
                    None
                }
            }
        });
        if out_stream.is_none() {
            crate::log("AUDIO: no render device — the wave is talk-only on this end");
        }

        // Park until the session ends; streams die with this scope.
        while ACTIVE.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        drop(in_stream);
        drop(out_stream);
        clear_queues();
        crate::log("AUDIO: session closed");
    }
}

#[cfg(not(any(target_os = "android", target_os = "redox")))]
pub use desktop::{route, start, stop};

// ---------------------------------------------------------------------------
// Android: Kotlin owns the device loops; JNI crosses into the shared queues.
// ---------------------------------------------------------------------------

#[cfg(target_os = "android")]
mod android {
    use super::*;

    /// Start: flip the flag, clear the queues, and ask the service to spin up AudioRecord/AudioTrack (VOICE_COMMUNICATION). Returns false when the service ref isn't up or the call fails — mic permission handling is Kotlin's side of the line.
    /// Start: flip the flag, clear the queues, let Kotlin do the Android-only chores (proximity lock, foreground microphone type, lock-screen flags, the RECORD_AUDIO prompt), then open the AAudio streams from Rust (audio_aaudio). False when the output stream cannot open.
    /// THE HANDOVER LOCK (field 2026-09-15 16:54, Emma's Note 10: "doesn't turn her screen off"): start and stop each run as ONE unit — streams AND the Kotlin chores. The session-ownership fix serialized the AAudio streams thru their own lock, but the ringback's stop reached Kotlin's stopWaveAudio AFTER the engine's startWaveAudio had already returned early on the still-true running flag, so the chores went down under the live wave and never came back: no proximity lock (the screen stayed lit at the ear), no mic foreground type, the earpiece route cleared. Held across the whole sequence, a stop finishes tearing down before a start begins building up, and the start then sees a stopped service and runs its full body.
    static HANDOVER: Mutex<()> = Mutex::new(());

    pub fn start() -> bool {
        let _h = HANDOVER.lock().unwrap();
        if ACTIVE.swap(true, Ordering::SeqCst) {
            return true;
        }
        clear_queues();
        // The wave starts on the user's own pick when that device is here (Kotlin's routeWaveAudio).
        let _ = crate::platform::jni_android::wave_service_str("setPreferredRoute", &super::picked_route().unwrap_or_default());
        let _ = crate::platform::jni_android::wave_service_void("startWaveAudio");
        if crate::platform::audio_aaudio::start() {
            true
        } else {
            let _ = crate::platform::jni_android::wave_service_void("stopWaveAudio");
            ACTIVE.store(false, Ordering::SeqCst);
            false
        }
    }

    pub fn stop() {
        if super::end_sweep_holds_stop() {
            return; // the disconnect sweep is playing; its own stop follows the drain
        }
        let _h = HANDOVER.lock().unwrap();
        if ACTIVE.swap(false, Ordering::SeqCst) {
            crate::platform::audio_aaudio::stop();
            let _ = crate::platform::jni_android::wave_service_void("stopWaveAudio");
            clear_queues();
        }
    }

    /// JNI ingress (PhotonConnectionService.nativeMicGranted): the RECORD_AUDIO grant landed mid-wave — open the input leg.
    pub(crate) fn on_mic_granted() {
        if ACTIVE.load(Ordering::Relaxed) {
            crate::platform::audio_aaudio::ensure_input();
        }
    }

    /// Route on Android — mirrored down from Kotlin's AudioDeviceCallback (see `on_route_mirror`); Unknown until the first callback (the duck treats Unknown as Builtin, the safe default).
    static ANDROID_ROUTE: Mutex<AudioRoute> = Mutex::new(AudioRoute::Unknown);

    pub fn route() -> AudioRoute {
        *ANDROID_ROUTE.lock().unwrap()
    }

    /// JNI ingress (PhotonConnectionService.nativeAudioRoute): the routed output device changed. `kind` is the coarse route (0 speaker / 1 earpiece / 2 headset / 3 bluetooth / else unknown); `id` is the calibration-profile identity (device type + BT name when present).
    pub(crate) fn on_route_mirror(kind: i32, id: String) {
        let route = match kind {
            0 => AudioRoute::Speaker,
            1 => AudioRoute::Earpiece,
            2 | 3 => AudioRoute::Headset,
            _ => AudioRoute::Unknown,
        };
        *ANDROID_ROUTE.lock().unwrap() = route;
        crate::logf!("AUDIO: route mirror — {} ({})", id, kind);
        super::set_route_identity(id);
    }

    /// JNI ingress: STREAM_VOICE_CALL volume in dB (getStreamVolumeDb), mirrored at service start + on every volume change.
    pub(crate) fn on_volume_mirror(db: f32) {
        super::set_volume_db(Some(db));
    }

    /// JNI ingress: the routed INPUT (mic) identity — the voice-profile key.
    pub(crate) fn on_mic_mirror(id: String) {
        crate::logf!("AUDIO: mic mirror — {}", id);
        super::set_mic_identity(id);
    }
}


#[cfg(target_os = "android")]
pub use android::{route, start, stop};
#[cfg(target_os = "android")]
pub(crate) use android::{on_mic_granted, on_mic_mirror, on_route_mirror, on_volume_mirror};

// Redox: no audio backend yet — waves are signaling-only there.
#[cfg(target_os = "redox")]
pub fn start() -> bool {
    false
}
#[cfg(target_os = "redox")]
pub fn stop() {}
#[cfg(target_os = "redox")]
pub fn route() -> AudioRoute {
    AudioRoute::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ONE test on purpose: the queues are process-global statics, so parallel test threads racing clear_queues corrupt each other — everything that touches them serializes here.
    #[test]
    fn queue_core_bounds_silence_and_reference() {
        // Bounded drop-oldest capture.
        clear_queues();
        for i in 0..(CAPTURE_Q_MAX + 10) {
            push_captured(0, i as i64 * FRAME_SAMPLES as i64, vec![i as i32; FRAME_SAMPLES]);
        }
        let drained = captured_frames();
        assert_eq!(drained.len(), CAPTURE_Q_MAX);
        // Oldest were dropped: the first surviving frame is #10, and its stream position rides with it.
        assert_eq!(drained[0].2[0], 10);
        assert_eq!(drained[0].1, 10 * FRAME_SAMPLES as i64);

        // LOCAL SOURCES play their queue verbatim: in order, exactly once, silence when dry — no priming, no splice, no trims.
        clear_queues();
        let s = next_render_frame();
        assert_eq!(s[0], 0, "a dry queue renders silence");
        queue_playback(vec![100i16; FRAME_SAMPLES]);
        let a = next_render_frame();
        assert_eq!(a, vec![100i16; FRAME_SAMPLES], "the first frame plays at once and whole");
        queue_playback(vec![101i16; FRAME_SAMPLES]);
        let b = next_render_frame();
        assert_eq!(b[0], 101, "then the next in order");
        let c = next_render_frame();
        assert_eq!(c[0], 0, "dry again: silence, never a guess");
        let r = render_reference();
        assert_eq!(r.len(), 4, "every rendered frame — silence or real — lands in the AEC reference");
        assert!(r[0].0 <= r[3].0, "reference is eagle-stamped in order");
        assert!(far_level() > 0, "far_level tracks rendered energy for the duck");
        // The learner's envelope tap: every rendered frame (silence AND real) appended as (osc, env), cursor drain is exactly-once, and the cursor base survives hygiene (a gap, never a replay).
        let (entries, cur) = render_env_since(0);
        assert_eq!(entries.len().min(4), 4, "all four rendered frames tapped (silence env 0.0 included)");
        let tail = &entries[entries.len() - 4..];
        assert_eq!(tail[0].1, 0.0, "dry-queue silence lands as env 0.0 — the true DAC signal");
        assert!(tail[1].1 > 0.0 && tail[2].1 > 0.0, "real frames carry their envelope");
        assert_eq!(tail[3].1, 0.0, "underrun silence lands as env 0.0");
        let (none, cur2) = render_env_since(cur);
        assert!(none.is_empty(), "cursor drain is exactly-once");
        assert_eq!(cur, cur2);

        clear_queues();
        assert_eq!(far_level(), 0, "clear_queues resets the duck's far-end signal");
        let (after_clear, cur3) = render_env_since(cur);
        assert!(after_clear.is_empty(), "hygiene clears the ring without rewinding the cursor base");
        assert_eq!(cur3, cur, "TOTAL survives clear — a held cursor sees a gap, never a phantom replay");
        queue_playback(vec![55i16; FRAME_SAMPLES]);
        let _ = next_render_frame();
        let (fresh, _) = render_env_since(cur);
        assert_eq!(fresh.len(), 1, "post-hygiene renders resume flowing to the held cursor");

        // NAMED PLAYOUT (as amended 2026-09-28): the far channel plays by name; l, the actual latency, follows L, the target, by ONE sample per zero-slope or sign-change point — never a jump.
        clear_queues();
        set_named_playout(true);
        let f = FRAME_SAMPLES as i64;
        set_play_target(f); // L = one frame
        let k: i64 = 1_000_000 * f;
        let dac = |dac_k: i64| vsf::grid::sample_to_eagle(dac_k);
        // On target, a present frame plays whole and exactly; an absent one is its own silence and a miss.
        queue_named(k, vec![100; FRAME_SAMPLES]);
        assert_eq!(next_render_frame_at(dac(k + f)), vec![100i16; FRAME_SAMPLES], "present and due: played whole");
        assert_eq!(next_render_frame_at(dac(k + 2 * f)), vec![0i16; FRAME_SAMPLES], "absent: its own silence, nothing repeated or faded");
        assert_eq!(jitter_stats().2, 1, "and it counts as a miss");
        assert_eq!(play_latency(), Some(f), "l held on L");
        // L drops by 100 samples: across SILENCE every sample is a zero-slope point, so l walks down one per sample and meets L inside the frame.
        set_play_target(f - 100);
        let _ = next_render_frame_at(dac(k + 3 * f));
        assert_eq!(play_latency(), Some(f - 100), "silence lets l meet L at once");
        // Speech-like audio (a 1 kHz tone, two peaks per 48 samples, no flat runs): l moves exactly one sample per peak or trough.
        let tone = |n: i64| (8000.0 * (2.0 * std::f64::consts::PI * n as f64 / 48.0 + 0.3).sin()) as i16;
        let start = PLAY_NEXT.load(Ordering::Relaxed);
        for fr in 0..4 {
            let k0 = start + fr * f;
            queue_named(k0, (k0..k0 + f).map(tone).collect());
        }
        set_play_target(f - 150); // 50 samples shorter than l
        let before = play_latency().unwrap();
        let _ = next_render_frame_at(dac(k + 4 * f));
        let moved = before - play_latency().unwrap();
        assert!((8..=12).contains(&moved), "one slip per peak or trough: {moved} in a frame holding ~10 of them");
        // l too SHORT: repeats at the same points, lengthening l one at a time.
        set_play_target(play_latency().unwrap() + 40);
        let before = play_latency().unwrap();
        let _ = next_render_frame_at(dac(k + 5 * f));
        let grew = play_latency().unwrap() - before;
        assert!((8..=12).contains(&grew), "repeats lengthen l one per point: {grew}");
        // TOO LATE: every name of a frame has had its instant — dropped at the door, never played.
        queue_named(k, vec![5; FRAME_SAMPLES]);
        assert_eq!(jitter_stats().3, 1, "counted as too late");
        // A CLOCK STEP RE-ANCHOR: the far names jump six seconds ahead; the engine restarts L from the stepped arrival and forgets the cursor, and the next render is the first frame again — exactly on the new L, no walk.
        let stepped = PLAY_NEXT.load(Ordering::Relaxed) + 6 * 48_000;
        queue_named(stepped, vec![77; FRAME_SAMPLES]);
        set_play_target(-6 * 48_000 + f); // the stepped arrival alone sets L (names ahead of our clock read as a negative age)
        reanchor_playout();
        assert_eq!(reanchors(), 1);
        let dac_k = stepped + (-6 * 48_000 + f); // the DAC instant whose first-frame cursor (dac − L) lands exactly on the stepped name
        let out = next_render_frame_at(dac(dac_k));
        assert_eq!(out, vec![77i16; FRAME_SAMPLES], "the first frame after a re-anchor starts on the new L and plays the stepped frame whole");
        assert_eq!(play_latency(), Some(-6 * 48_000 + f), "l is the new L at once — a jump by the first-arrival rule, not a walk");
        // THE RENDER TRACE: armed, every named far frame lands in the ring as the DAC got it, with its name and holes.
        set_profile(true);
        let _ = take_render_trace();
        let p0 = PLAY_NEXT.load(Ordering::Relaxed);
        queue_named(p0, vec![31; FRAME_SAMPLES]);
        let _ = next_render_frame_at(dac(p0 + play_latency().unwrap()));
        let tr = take_render_trace();
        assert_eq!(tr.len(), 1, "one traced frame per render");
        assert_eq!(tr[0].name, p0, "traced under the name it started on");
        assert_eq!(tr[0].missing, 0);
        assert_eq!(tr[0].pcm, vec![31i16; FRAME_SAMPLES], "the trace is the DAC's frame");
        set_profile(false);
        assert!(take_render_trace().is_empty(), "disarmed: nothing traced");
        clear_queues();
        assert_eq!(reanchors(), 0, "reset with the rest");
        // THE FOLLOWER: fast up, seven-eighths down — a step settles in frames, not instantly, and the way down is the slower.
        let mut up = 0i64;
        for _ in 0..3 {
            up = follow(up, 4096);
        }
        assert_eq!(up, 3584, "three frames up from zero reach 7/8 of a 4096 step");
        let mut down = 4096i64;
        for _ in 0..3 {
            down = follow(down, 0);
        }
        assert!(down > 2048 && down < 3000, "three frames down from 4096 keep over half of it ({down})");
        let mut steady = 100i64;
        for _ in 0..200 {
            steady = follow(steady, 100);
        }
        assert_eq!(steady, 100, "a steady input is passed exactly");
        // THE RAMP: a frame whose target gain steps from unity to zero walks there one sample at a time — no staircase edge.
        clear_queues();
        set_speaker_duck(true);
        // Armed with k at its neutral seed (1/16) the loss term alone is a quarter: the first frame walks from the unity it inherited down to that, never a step.
        queue_playback(vec![10000i16; FRAME_SAMPLES]);
        let first = next_render_frame();
        assert!(first[0] > 9900, "the first sample is still at the inherited unity ({})", first[0]);
        assert!((2480..=2520).contains(&first[FRAME_SAMPLES - 1]), "the last sample has reached the quarter loss ({})", first[FRAME_SAMPLES - 1]);
        assert!(first.windows(2).all(|w| w[0] >= w[1]), "the walk is monotonic — one step per sample, never a jump");
        note_near_level(1 << 20); // a near level past full duck: the duck's target is zero
        queue_playback(vec![10000i16; FRAME_SAMPLES]);
        let ramp = next_render_frame();
        assert!((2400..=2520).contains(&ramp[0]), "the next frame starts where the last one ended ({})", ramp[0]);
        assert!(ramp[FRAME_SAMPLES - 1] <= 1, "the last sample has reached the zero target ({})", ramp[FRAME_SAMPLES - 1]);
        assert!(ramp.windows(2).all(|w| w[0] >= w[1]), "the walk is monotonic — one step per sample, never a jump");
        queue_playback(vec![10000i16; FRAME_SAMPLES]);
        let held = next_render_frame();
        assert!(held.iter().all(|&s| s <= 1), "the next frame starts where the ramp ended");
        clear_queues();
    }
}
