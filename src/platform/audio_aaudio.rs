//! Android call audio on AAudio, owned by Rust (2026-09-09, the OboeTester baseline): the Java AudioRecord/AudioTrack path could not reach the MMAP exclusive fast path at all — a 60 ms record buffer, a 20 ms track and the shared mixer put our speaker→mic loop north of 100 ms on a phone that measures 20.6 ms round trip in exclusive mode. Two AAudio streams in EXCLUSIVE + LOW_LATENCY (shared as the fallback), 48 kHz mono I16, callback-driven at the HAL's own burst (2 ms on that phone), and every frame stamped with the HAL's presentation/capture timestamp instead of our enqueue/arrival moment — so the chirp, the learner, the duck and the canceller all live on one physical timeline.
//!
//! Kotlin keeps only what Android makes Kotlin's: the proximity lock, the foreground-service microphone type, the lock-screen flags, the RECORD_AUDIO prompt (whose grant calls `nativeMicGranted` → [`ensure_input`]), and the route/volume mirrors.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use ndk::audio::{
    AudioCallbackResult, AudioDirection, AudioFormat, AudioPerformanceMode, AudioSharingMode, AudioStream, AudioStreamBuilder, Clockid,
};

use super::audio::{next_render_frame_at, push_captured, FRAME_SAMPLES};

const SAMPLE_RATE: i32 = 48_000;

/// AAudio stream handles are safe to control from any thread (the NDK contract); the callbacks are `Send` closures already.
struct SendStream(AudioStream);
unsafe impl Send for SendStream {}

struct Session {
    output: SendStream,
    input: Option<SendStream>,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
/// A stream reported an error (route change, device gone): rebuilt off the callback thread, once per fault.
static REOPENING: AtomicBool = AtomicBool::new(false);

/// CLOCK_MONOTONIC now, in nanoseconds — the clock AAudio timestamps are on.
fn monotonic_ns() -> i64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as i64 * 1_000_000_000 + ts.tv_nsec as i64
}

/// Map a monotonic instant to eagle oscillations thru a fresh (now, now) pair — the two clocks are read back to back, so the mapping error is the read skew, microseconds.
fn eagle_at_monotonic(target_ns: i64) -> i64 {
    let eagle_now = vsf::eagle_time_oscillations();
    let mono_now = monotonic_ns();
    let delta_ns = target_ns - mono_now;
    eagle_now + delta_ns * vsf::OSCILLATIONS_PER_SECOND as i64 / 1_000_000_000
}

/// The eagle time at which frame `pos` of this stream hits the DAC (output) or left the ADC (input), from the HAL's latest timestamp; None before the HAL has one (the first few bursts).
fn frame_time(stream: &AudioStream, pos: i64) -> Option<i64> {
    let ts = stream.timestamp(Clockid::Monotonic).ok()?;
    let ns = ts.time_nanoseconds + (pos - ts.frame_position) * 1_000_000_000 / SAMPLE_RATE as i64;
    Some(eagle_at_monotonic(ns))
}

/// Build one stream. AAudio's DEFAULTS are the attributes we want — MEDIA usage rides the fast mixer (the vendor voice pipeline behind VOICE_COMMUNICATION was the 80 ms floor, 2026-08-19) and the VOICE_RECOGNITION preset is the mic without the vendor NS/AGC/AEC chain — and the explicit setters are API 28 while minSdk is 26, so nothing is set. The callback box is consumed by the builder, so a failed open needs a fresh one from the factory.
/// Whether the output opens with the voice-communication usage (the earpiece route's label). Field 2026-09-12: Nick's Pixel kept Exclusive/LowLatency at 4 ms under it, Emma's phone came up Shared/None at 40 ms on every wave — the vendor policy there hands voice-usage streams to its voice pipeline. So the usage is tried first and DROPPED for the process when it costs the fast path (the loudspeaker at 4 ms beats the earpiece at 40).
static VOICE_USAGE_OK: AtomicBool = AtomicBool::new(true);

fn build(direction: AudioDirection, sharing: AudioSharingMode, cb: ndk::audio::AudioStreamDataCallback) -> Result<AudioStream, String> {
    // VOICE USAGE ON THE OUTPUT (Nick 2026-09-12, "exclusive earpiece without losing latency"): the earpiece route (Kotlin's setCommunicationDevice) attaches to voice-usage streams, and usage is only a policy label — sharing mode, performance mode and burst size are set here and untouched by it. The August 80 ms floor came from the IN_COMMUNICATION audio MODE waking the vendor voice pipeline; the mode is never entered. The "AAudio out up" line is the proof: Exclusive/LowLatency at 4 ms, or it isn't.
    let b = AudioStreamBuilder::new()
        .map_err(|e| format!("builder: {e:?}"))?
        .direction(direction)
        .usage(if matches!(direction, AudioDirection::Output) && VOICE_USAGE_OK.load(Ordering::Relaxed) { ndk::audio::AudioUsage::VoiceCommunication } else { ndk::audio::AudioUsage::Media })
        .sharing_mode(sharing)
        .performance_mode(AudioPerformanceMode::LowLatency)
        .sample_rate(SAMPLE_RATE)
        .channel_count(1)
        .format(AudioFormat::PCM_I16)
        .data_callback(cb)
        .error_callback(Box::new(move |_s, e| on_stream_error(direction, e)));
    b.open_stream().map_err(|e| format!("{sharing:?}: {e:?}"))
}

/// Exclusive first (the MMAP fast path the tester measured at 20 ms), shared as the fallback.
fn open_with_fallback(direction: AudioDirection, make_cb: &dyn Fn() -> ndk::audio::AudioStreamDataCallback) -> Result<AudioStream, String> {
    match build(direction, AudioSharingMode::Exclusive, make_cb()) {
        Ok(s) => Ok(s),
        Err(first) => {
            crate::logf!("AUDIO: AAudio {} exclusive open failed ({}) — falling back to shared", format!("{direction:?}"), first);
            build(direction, AudioSharingMode::Shared, make_cb())
        }
    }
}

fn output_callback() -> ndk::audio::AudioStreamDataCallback {
    // Position of the next frame this callback writes; `leftover` = the tail of the last popped 5 ms render frame not yet written.
    let mut pos: i64 = 0;
    let mut leftover: Vec<i16> = Vec::with_capacity(FRAME_SAMPLES);
    let mut leftover_at: usize = 0;
    Box::new(move |stream: &AudioStream, data: *mut c_void, num_frames: i32| {
        let n = num_frames.max(0) as usize;
        let out = unsafe { std::slice::from_raw_parts_mut(data as *mut i16, n) };
        let mut written = 0usize;
        while written < n {
            if leftover_at >= leftover.len() {
                // The frame about to start at `pos + written` hits the DAC at the HAL's time for that position — the stamp the reference ring, the learner and the chirp anchor read.
                let at = frame_time(stream, pos + written as i64).unwrap_or_else(vsf::eagle_time_oscillations);
                leftover = next_render_frame_at(at);
                leftover_at = 0;
                if leftover.is_empty() {
                    leftover = vec![0i16; FRAME_SAMPLES];
                }
            }
            let take = (leftover.len() - leftover_at).min(n - written);
            out[written..written + take].copy_from_slice(&leftover[leftover_at..leftover_at + take]);
            leftover_at += take;
            written += take;
        }
        pos += n as i64;
        AudioCallbackResult::Continue
    })
}

fn input_callback() -> ndk::audio::AudioStreamDataCallback {
    let mut pos: i64 = 0;
    let mut acc: Vec<i16> = Vec::with_capacity(FRAME_SAMPLES * 2);
    let mut acc_start: i64 = 0;
    Box::new(move |stream: &AudioStream, data: *mut c_void, num_frames: i32| {
        let n = num_frames.max(0) as usize;
        let samples = unsafe { std::slice::from_raw_parts(data as *const i16, n) };
        if acc.is_empty() {
            acc_start = pos;
        }
        acc.extend_from_slice(samples);
        pos += n as i64;
        while acc.len() >= FRAME_SAMPLES {
            let frame: Vec<i16> = acc.drain(..FRAME_SAMPLES).collect();
            // The stamp is when this frame's FIRST sample left the ADC — the HAL's clock, not our arrival.
            let at = frame_time(stream, acc_start).unwrap_or_else(vsf::eagle_time_oscillations);
            push_captured(at, frame);
            acc_start += FRAME_SAMPLES as i64;
        }
        AudioCallbackResult::Continue
    })
}

fn describe(stream: &AudioStream, what: &str) {
    crate::logf!(
        "AUDIO: AAudio {} up — {}/{}, burst {} fr, buffer {} fr ({} ms), rate {}",
        what,
        format!("{:?}", stream.sharing_mode()),
        format!("{:?}", stream.performance_mode()),
        stream.frames_per_burst(),
        stream.buffer_size_in_frames(),
        stream.buffer_size_in_frames() as i64 * 1000 / SAMPLE_RATE as i64,
        stream.sample_rate()
    );
}

fn start_output() -> Result<AudioStream, String> {
    let mut s = open_with_fallback(AudioDirection::Output, &output_callback)?;
    // THE GUARD (2026-09-12): the earpiece is only free when the voice-usage stream still runs the fast path. A stream that came up without LowLatency has been claimed by the vendor voice pipeline — close it, forget voice usage for this process, and reopen on media (the loudspeaker; Kotlin's earpiece route then has nothing to attach to and the OS falls back on its own).
    if VOICE_USAGE_OK.load(Ordering::Relaxed) && !matches!(s.performance_mode(), AudioPerformanceMode::LowLatency) {
        crate::logf!("AUDIO: voice usage cost the fast path on this device ({}/{}, {} fr bursts) — reopening on media usage, loudspeaker", format!("{:?}", s.sharing_mode()), format!("{:?}", s.performance_mode()), s.frames_per_burst());
        VOICE_USAGE_OK.store(false, Ordering::Relaxed);
        drop(s);
        s = open_with_fallback(AudioDirection::Output, &output_callback)?;
    }
    // Two bursts of buffer: the smallest ask that still absorbs one late callback (the tester's own default).
    let _ = s.set_buffer_size_in_frames(s.frames_per_burst() * 2);
    s.request_start().map_err(|e| format!("start: {e:?}"))?;
    describe(&s, "out");
    Ok(s)
}

fn start_input() -> Result<AudioStream, String> {
    let s = open_with_fallback(AudioDirection::Input, &input_callback)?;
    let _ = s.set_buffer_size_in_frames(s.frames_per_burst() * 2);
    s.request_start().map_err(|e| format!("start: {e:?}"))?;
    describe(&s, "in");
    Ok(s)
}

/// Open and start both streams. The input needs RECORD_AUDIO: without it the open fails, Kotlin has already prompted, and the grant lands thru [`ensure_input`]. Returns false only when the OUTPUT cannot open — a call without a speaker is no call.
pub fn start() -> bool {
    let mut g = SESSION.lock().unwrap();
    if g.is_some() {
        return true;
    }
    let output = match start_output() {
        Ok(s) => s,
        Err(e) => {
            crate::logf!("AUDIO: AAudio output failed — {}", e);
            return false;
        }
    };
    let input = match start_input() {
        Ok(s) => Some(s),
        Err(e) => {
            crate::logf!("AUDIO: AAudio input not open ({}) — listen-only until the mic grant lands", e);
            None
        }
    };
    *g = Some(Session { output: SendStream(output), input: input.map(SendStream) });
    true
}

/// The mic grant landed mid-call (Kotlin → nativeMicGranted): open the input leg the missing permission skipped.
pub fn ensure_input() {
    let mut g = SESSION.lock().unwrap();
    let Some(s) = g.as_mut() else { return };
    if s.input.is_some() {
        return;
    }
    match start_input() {
        Ok(i) => s.input = Some(SendStream(i)),
        Err(e) => crate::logf!("AUDIO: AAudio input still not open after the grant — {}", e),
    }
}

/// Stop and close both streams (Drop closes). Logs the HAL's underrun/overrun counts — the buffer-floor monitor.
pub fn stop() {
    let taken = SESSION.lock().unwrap().take();
    let Some(s) = taken else { return };
    let out_x = s.output.0.x_run_count();
    let in_x = s.input.as_ref().map(|i| i.0.x_run_count()).unwrap_or(-1);
    let _ = s.output.0.request_stop();
    if let Some(i) = s.input.as_ref() {
        let _ = i.0.request_stop();
    }
    crate::logf!("AUDIO: AAudio down — out xruns {}, in xruns {}", out_x, in_x);
    drop(s);
}

/// A stream faulted (a headset plugged in, the device went away): the HAL disconnects exclusive streams on a route change. Rebuild off the callback thread — never close a stream from its own callback — once per fault, only while a call is live.
fn on_stream_error(direction: AudioDirection, e: ndk::audio::AudioError) {
    crate::logf!("AUDIO: AAudio {} stream error {} — rebuilding both streams", format!("{direction:?}"), format!("{e:?}"));
    if REOPENING.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new().name("aaudio-reopen".into()).spawn(|| {
        if super::audio::is_active() {
            stop();
            start();
        }
        REOPENING.store(false, Ordering::SeqCst);
    });
}
