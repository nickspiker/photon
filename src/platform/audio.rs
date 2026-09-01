//! Call audio I/O — the ONE capture/playback surface for voice calls (docs/calls.md), platform-split under a shared queue core.
//!
//! The call engine speaks 48kHz mono i16 in 10ms frames (480 samples) and never touches a device API: it drains `captured_frames()` and feeds `queue_playback()`. Under that:
//! - **Desktop**: a dedicated audio thread owns the cpal input+output streams (cpal streams are !Send — built and parked on their own thread, torn down when `stop()` clears the active flag). Device-rate/channel conversion happens at the callback edge via a naive linear resampler — correctness first; a better resampler is a drop-in.
//! - **Android**: Kotlin owns AudioRecord/AudioTrack and crosses JNI into the same queues: `nativeAudioCaptured` pushes mic frames, `nativeAudioNextFrame` pulls render frames. Both ends ride the LOW-LATENCY paths (capture: VOICE_RECOGNITION raw fast-track; render: USAGE_MEDIA fast mixer) — vendor voice-pipeline processing is deliberately OFF both ways (Nick, latency-first 2026-08-20), so echo control belongs to OUR canceller over RENDER_REF. Start/stop ride the MESSAGE_NOTIFIER service ref like notifications do.
//!
//! **AEC plumbing from day one** (the retrofit-misery lesson): every rendered sample lands in an eagle-stamped reference ring BEFORE it reaches the device, whether or not any canceller exists yet. When a canceller (or the suppression duck) arrives, its far-end reference is already exact — the decoded signal we handed the DAC, not a guess at what some stack played.
//!
//! Queues are bounded drop-oldest: realtime audio must never block and never balloon — a stalled consumer costs the oldest 10ms, not memory or latency.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

/// The call engine's sample rate — everything above the device edge is 48kHz mono.
pub const SAMPLE_RATE: u32 = 48_000;
/// 10ms @ 48kHz mono — the Opus frame the engine encodes.
pub const FRAME_SAMPLES: usize = 480;

/// Mic frames waiting for the engine (drop-oldest past ~500ms).
static CAPTURE_Q: Mutex<VecDeque<Vec<i16>>> = Mutex::new(VecDeque::new());
/// Decoded far-end frames waiting for the device (drop-oldest past ~1s).
static PLAYBACK_Q: Mutex<VecDeque<Vec<i16>>> = Mutex::new(VecDeque::new());
/// The AEC far-end reference: (eagle osc at enqueue-to-device, samples) per frame, last ~500ms. The canceller/duck reads this; nothing else does.
static RENDER_REF: Mutex<VecDeque<(i64, Vec<i16>)>> = Mutex::new(VecDeque::new());
/// Audio session live? Device loops run only while true.
static ACTIVE: AtomicBool = AtomicBool::new(false);

const CAPTURE_Q_MAX: usize = 50; // 500ms
const PLAYBACK_Q_MAX: usize = 100; // 1s
const RENDER_REF_MAX: usize = 50; // 500ms

// ADAPTIVE JITTER BUFFER (docs/calls.md): the far end arrives in bursts (a FEC window at a time) and the network jitters, so a fixed buffer either adds latency it doesn't need (clean LAN) or underruns (lossy relay).
// Instead the render side plays silence until the queue reaches `JITTER_TARGET` frames, then drains steadily; a dry queue (underrun) GROWS the target and re-primes, while a long clean stretch SHRINKS it back toward the floor.
// So a clean call rests at ~20ms of software buffer and only a jittery path pays more — exactly where the latency should go.
// This sits BEFORE the device buffer, which is kept shallow (low-latency AudioTrack), so this is the ONE place jitter is absorbed.
const JITTER_FLOOR: usize = 1; // 10ms — playback starts the instant the first frame exists; one frame of wobble tolerance. Zero is mechanically possible but useless: the queue is frame-quantized, so floor 0 saves at most one frame while making EVERY timing wobble an audible gap + re-prime stumble — and the adaptive growth would lift it right back. Below one frame the lever is smaller Opus frames (5ms CELT), not this constant.
const JITTER_CAP: usize = 12; // 120ms — the most we'll ever buffer, even on a bad relay
const JITTER_GROW: usize = 2; // frames added on each underrun
const JITTER_DECAY_FRAMES: usize = 500; // ~5s of clean playback before shrinking one step
static JITTER_TARGET: AtomicUsize = AtomicUsize::new(JITTER_FLOOR);
static JITTER_PRIMING: AtomicBool = AtomicBool::new(true);
static JITTER_CLEAN_STREAK: AtomicUsize = AtomicUsize::new(0);
// STANDING-DEPTH TRIM (2026-09-01 Emma/Nick field call): depth acquired during a transient (slow-start's 4-frame floor bursts, a recv-path stall) is PERMANENT without this — the DAC drains at exactly realtime, so excess queue = mouth-to-ear latency for the rest of the call. Target decay alone never sheds it (it only matters at a re-prime). When the queue sits above target+1 for a full clean second, drop ONE frame (10ms) and re-observe — sheds a standing 40ms in ~4s, inaudible against speech.
const TRIM_OVER_SLACK: usize = 1; // frames above target that count as "standing over"
const TRIM_OBSERVE_FRAMES: usize = 100; // 1s of consecutive over-depth before each single-frame drop
static TRIM_OVER_STREAK: AtomicUsize = AtomicUsize::new(0);
// Telemetry — the numbers that turn "latency feels a little off" into a diagnosis (peak standing depth vs target vs trims). Reset per call in clear_queues, logged by the engine at teardown.
static JITTER_UNDERRUNS: AtomicUsize = AtomicUsize::new(0);
static JITTER_DEPTH_PEAK: AtomicUsize = AtomicUsize::new(0);
static JITTER_TRIMS: AtomicUsize = AtomicUsize::new(0);

/// Per-call jitter diagnostics: (target_frames, current_depth, underruns, peak_depth, trims). 10ms per frame.
pub fn jitter_stats() -> (usize, usize, usize, usize, usize) {
    (
        JITTER_TARGET.load(Ordering::Relaxed),
        PLAYBACK_Q.lock().unwrap().len(),
        JITTER_UNDERRUNS.load(Ordering::Relaxed),
        JITTER_DEPTH_PEAK.load(Ordering::Relaxed),
        JITTER_TRIMS.load(Ordering::Relaxed),
    )
}

/// Peak-held mean |sample| of what the device is rendering (~80ms decay) — the engine's soft duck reads this as the far-end activity signal, covering the device-buffer + acoustic lag without sample-accurate alignment.
static FAR_LEVEL: AtomicUsize = AtomicUsize::new(0);

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

/// Drain every captured frame since the last call (10ms 48kHz mono each). Engine-side, any thread.
pub fn captured_frames() -> Vec<Vec<i16>> {
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
    JITTER_DEPTH_PEAK.fetch_max(q.len(), Ordering::Relaxed);
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

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

fn push_captured(frame: Vec<i16>) {
    let mut q = CAPTURE_Q.lock().unwrap();
    if q.len() >= CAPTURE_Q_MAX {
        q.pop_front();
    }
    q.push_back(frame);
}

/// Pop the next render frame through the adaptive jitter buffer (silence when priming or dry — the no-PLC doctrine: missing audio is silence, never guesswork) and log it into the reference ring. Single-consumer (the one render loop), so the jitter atomics need no CAS.
fn next_render_frame() -> Vec<i16> {
    let silence = || vec![0i16; FRAME_SAMPLES];
    let frame = {
        let mut q = PLAYBACK_Q.lock().unwrap();
        if JITTER_PRIMING.load(Ordering::Relaxed) {
            // Building depth: play silence until the queue reaches the target, then start draining.
            if q.len() >= JITTER_TARGET.load(Ordering::Relaxed) {
                JITTER_PRIMING.store(false, Ordering::Relaxed);
                q.pop_front().unwrap_or_else(silence)
            } else {
                silence()
            }
        } else {
            match q.pop_front() {
                Some(f) => {
                    // Clean drain: after a long steady stretch, shrink the target one step toward the floor.
                    let streak = JITTER_CLEAN_STREAK.fetch_add(1, Ordering::Relaxed) + 1;
                    let target = JITTER_TARGET.load(Ordering::Relaxed);
                    if streak >= JITTER_DECAY_FRAMES && target > JITTER_FLOOR {
                        JITTER_TARGET.store(target - 1, Ordering::Relaxed);
                        JITTER_CLEAN_STREAK.store(0, Ordering::Relaxed);
                    }
                    // Standing-depth trim (see the consts): a queue persistently above target is latency, not safety — after a full second of over-depth, drop one frame to close the gap.
                    if q.len() > target + TRIM_OVER_SLACK {
                        let over = TRIM_OVER_STREAK.fetch_add(1, Ordering::Relaxed) + 1;
                        if over >= TRIM_OBSERVE_FRAMES {
                            q.pop_front();
                            JITTER_TRIMS.fetch_add(1, Ordering::Relaxed);
                            TRIM_OVER_STREAK.store(0, Ordering::Relaxed);
                        }
                    } else {
                        TRIM_OVER_STREAK.store(0, Ordering::Relaxed);
                    }
                    f
                }
                None => {
                    // Underrun: grow the target (capped), reset the clean streak, and re-prime.
                    let target = JITTER_TARGET.load(Ordering::Relaxed);
                    JITTER_TARGET.store((target + JITTER_GROW).min(JITTER_CAP), Ordering::Relaxed);
                    JITTER_CLEAN_STREAK.store(0, Ordering::Relaxed);
                    JITTER_PRIMING.store(true, Ordering::Relaxed);
                    JITTER_UNDERRUNS.fetch_add(1, Ordering::Relaxed);
                    silence()
                }
            }
        }
    };
    // Far-end level for the duck: peak-hold with a per-frame decay (~80ms fall from full), so the mic stays attenuated across the device-buffer + acoustic lag rather than only the exact rendered instant.
    let lvl = (frame.iter().map(|s| s.unsigned_abs() as u64).sum::<u64>() / FRAME_SAMPLES as u64) as usize;
    let old = FAR_LEVEL.load(Ordering::Relaxed);
    FAR_LEVEL.store(lvl.max(old - old / 4), Ordering::Relaxed);
    {
        let mut r = RENDER_REF.lock().unwrap();
        if r.len() >= RENDER_REF_MAX {
            r.pop_front();
        }
        r.push_back((vsf::eagle_time_oscillations(), frame.clone()));
    }
    frame
}

/// Reset all queues — session start/stop hygiene so a new call never hears the last call's tail.
/// Logs the session's jitter diagnostics FIRST (this runs at both start and stop; the stop edge is the one whose numbers matter, and a start against zeroed stats logs nothing).
fn clear_queues() {
    let (target, depth, underruns, peak, trims) = jitter_stats();
    if underruns > 0 || peak > 0 || trims > 0 {
        crate::logf!(
            "CALL: jitter — target {} depth {} peak {} underruns {} trims {} (frames, 10ms each)",
            target,
            depth,
            peak,
            underruns,
            trims
        );
    }
    CAPTURE_Q.lock().unwrap().clear();
    PLAYBACK_Q.lock().unwrap().clear();
    RENDER_REF.lock().unwrap().clear();
    // Each call starts fresh at the jitter floor, re-priming — never inheriting the last call's grown depth.
    JITTER_TARGET.store(JITTER_FLOOR, Ordering::Relaxed);
    JITTER_PRIMING.store(true, Ordering::Relaxed);
    JITTER_CLEAN_STREAK.store(0, Ordering::Relaxed);
    TRIM_OVER_STREAK.store(0, Ordering::Relaxed);
    JITTER_UNDERRUNS.store(0, Ordering::Relaxed);
    JITTER_DEPTH_PEAK.store(0, Ordering::Relaxed);
    JITTER_TRIMS.store(0, Ordering::Relaxed);
    FAR_LEVEL.store(0, Ordering::Relaxed);
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

    /// Start the audio session: spawns the thread that owns both cpal streams for the life of the call. Returns false when no devices exist (the call proceeds one-way rather than failing — a mic-less desktop can still listen).
    pub fn start() -> bool {
        if ACTIVE.swap(true, Ordering::SeqCst) {
            return true; // already live
        }
        clear_queues();
        std::thread::Builder::new()
            .name("call-audio".into())
            .spawn(audio_thread)
            .is_ok()
    }

    pub fn stop() {
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
        let stream = dev
            .build_input_stream(
                &cfg,
                move |data: &[T], _: &cpal::InputCallbackInfo| {
                    let as_f32: Vec<f32> = data.iter().map(|s| s.to_sample::<f32>()).collect();
                    let mono = fold_mono(&as_f32, channels);
                    let mut out = Vec::with_capacity(mono.len());
                    rs.run(&mono, &mut out);
                    pending.extend(out);
                    while pending.len() >= FRAME_SAMPLES {
                        let frame: Vec<i16> = pending
                            .drain(..FRAME_SAMPLES)
                            .map(|s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
                            .collect();
                        push_captured(frame);
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
                move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                    let ch = channels.max(1);
                    let need_mono = data.len() / ch;
                    while staged.len() < need_mono {
                        let frame = next_render_frame();
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
            crate::log("AUDIO: no capture device — call is listen-only on this end");
        }

        // ---- output (speaker) ----
        let out_stream = host.default_output_device().and_then(|dev| {
            let name = dev.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "?".into());
            *ROUTE.lock().unwrap() = sniff_route(&name);
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
            crate::log("AUDIO: no render device — call is talk-only on this end");
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
    pub fn start() -> bool {
        if ACTIVE.swap(true, Ordering::SeqCst) {
            return true;
        }
        clear_queues();
        if crate::platform::jni_android::call_service_void("startCallAudio") {
            true
        } else {
            ACTIVE.store(false, Ordering::SeqCst);
            false
        }
    }

    pub fn stop() {
        if ACTIVE.swap(false, Ordering::SeqCst) {
            let _ = crate::platform::jni_android::call_service_void("stopCallAudio");
            clear_queues();
        }
    }

    /// Route on Android: Kotlin mirrors it on device/route callbacks; Unknown until told. (Wired in the Kotlin audio step; the duck treats Unknown as Builtin.)
    pub fn route() -> AudioRoute {
        AudioRoute::Unknown
    }

    /// JNI ingress: one mic frame from Kotlin's AudioRecord loop.
    pub(crate) fn on_captured(frame: Vec<i16>) {
        if ACTIVE.load(Ordering::Relaxed) {
            push_captured(frame);
        }
    }

    /// JNI egress: next frame for Kotlin's AudioTrack loop (silence when dry).
    pub(crate) fn pull_render() -> Vec<i16> {
        next_render_frame()
    }
}

#[cfg(target_os = "android")]
pub use android::{route, start, stop};
#[cfg(target_os = "android")]
pub(crate) use android::{on_captured, pull_render};

// Redox: no audio backend yet — calls are signaling-only there.
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
            push_captured(vec![i as i16; FRAME_SAMPLES]);
        }
        let drained = captured_frames();
        assert_eq!(drained.len(), CAPTURE_Q_MAX);
        // Oldest were dropped: the first surviving frame is #10.
        assert_eq!(drained[0][0], 10);

        // Adaptive jitter buffer: renders silence while PRIMING (queue below the floor), drains real frames once the floor is reached, and a dry queue underruns to silence (never PLC guesswork). Every rendered frame — silence or real — feeds the AEC reference.
        clear_queues(); // resets the jitter state: priming, target = JITTER_FLOOR
        assert_eq!(JITTER_FLOOR, 1, "this test assumes a 1-frame floor");
        // Empty queue below the floor → still priming → silence.
        let s = next_render_frame();
        assert_eq!(s[0], 0, "priming below the jitter floor renders silence");
        // The FIRST frame reaches the floor and plays immediately — the whole point of floor 1: zero added hold on a clean chain.
        queue_playback(vec![100i16; FRAME_SAMPLES]);
        let a = next_render_frame();
        assert_eq!(a[0], 100, "at the floor, the first frame plays immediately");
        queue_playback(vec![101i16; FRAME_SAMPLES]);
        let b = next_render_frame();
        assert_eq!(b[0], 101, "then the next in order");
        // Now dry → underrun → silence (never PLC guesswork).
        let c = next_render_frame();
        assert_eq!(c[0], 0, "dry buffer renders silence, never PLC guesswork");
        let r = render_reference();
        assert_eq!(r.len(), 4, "every rendered frame — silence or real — lands in the AEC reference");
        assert!(r[0].0 <= r[3].0, "reference is eagle-stamped in order");
        // The duck's far-end signal: rendering real frames raised it (peak-hold survives the one silent frame), and session hygiene zeroes it.
        assert!(far_level() > 0, "far_level tracks rendered energy for the duck");
        clear_queues();
        assert_eq!(far_level(), 0, "clear_queues resets the duck's far-end signal");
    }
}
