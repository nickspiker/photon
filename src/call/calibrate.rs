//! Per-route audio calibration, TWO independent measurements (Nick 2026-09-02 — the single combined flow read as vague because it smuggled two different measurements thru one button):
//!
//! **Echo check** (speaker → mic): plays the recorded fox sentence thru the CURRENT OUTPUT while the user stays quiet; cross-correlating what the mic heard against what was played yields the render→capture DELAY and the coupling gain `g` — the leak your friends would hear as echo. Property of the OUTPUT ROUTE (`audio.cal.echo.<route>`): speakerphone leaks hugely, an earpiece barely, a headset ~zero.
//!
//! **Voice check** (mouth → mic): plays the same sentence once as the example, then records the user repeating it; the trimmed median of voiced frames sets a FIXED mic gain, the quiet gaps the noise floor. Property of the MIC + user (`audio.cal.voice.<mic>`) — switching speaker→earpiece keeps the voice profile (same mic), a BT headset needs both (its own mic).
//!
//! Each step re-runs independently; results carry human-readable verdicts so the page reads like an instrument, not a ritual. Session ownership mirrors playback.rs (refuse while a call owns audio); phases are edges. Stability gates void a run when the volume moves, the route swaps, or the mic changes mid-measurement — a stored profile is a MEASUREMENT, never a guess.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

/// The embedded prompt: PHPRMPT1 framing (magic + [len u16 LE][opus packet]…), minted by `promptenc` from the recorded master at the call's top rung (48k mono CELT LowDelay @128k, 10ms frames).
const PROMPT_ASSET: &[u8] = include_bytes!("../../assets/cal-prompt.opusp");
const FRAME_SAMPLES: usize = 480; // 10ms @ 48k — matches platform::audio

/// Where a running measurement stands — published for the Wave page to render live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalPhase {
    Idle = 0,
    /// Echo check: the prompt is playing; the user must stay quiet (the measurement).
    EchoListen = 1,
    /// Voice check: the example sentence is playing — listen.
    VoiceExample = 2,
    /// Voice check: the mic window is open — repeat the sentence now.
    VoiceRepeat = 3,
    /// A run finished and posted its result.
    Done = 4,
    /// A gate failed — nothing stored; the page says try again.
    Failed = 5,
}

static PHASE: AtomicU8 = AtomicU8::new(0);

/// The UI wake for phase edges. Phases are edges and the event loop is event-driven — a transition that doesn't wake the loop paints only when something ELSE stirs it (field 2026-09-02: "hang on" sat on the Wave page forever; Done was long since stored, the screen just never repainted until a scroll). Registered once by the app (the same law as the persist writer's verdict wake, messaging.rs 2026-08-25); every phase transition fires it and the status tick's drain does the rest.
static WAKE: std::sync::OnceLock<Box<dyn Fn() + Send + Sync>> = std::sync::OnceLock::new();

/// Register the event-loop wake fired on every calibration phase transition. First registration wins; later calls are no-ops (one loop, one wake).
pub fn register_wake(f: impl Fn() + Send + Sync + 'static) {
    let _ = WAKE.set(Box::new(f));
}

/// EVERY phase transition goes thru here: store, then wake the loop so the transition paints now, not at the next unrelated event.
fn set_phase(p: CalPhase) {
    PHASE.store(p as u8, Ordering::Relaxed);
    if let Some(w) = WAKE.get() {
        w();
    }
}

pub fn phase() -> CalPhase {
    match PHASE.load(Ordering::Relaxed) {
        1 => CalPhase::EchoListen,
        2 => CalPhase::VoiceExample,
        3 => CalPhase::VoiceRepeat,
        4 => CalPhase::Done,
        5 => CalPhase::Failed,
        _ => CalPhase::Idle,
    }
}

/// The echo-path profile: how much of this OUTPUT leaks back into the mic, and how late.
#[derive(Debug, Clone, PartialEq)]
pub struct EchoProfile {
    /// Coupling gain, volume-normalized (captured ÷ rendered at the correlation peak ÷ vol_lin at measure time).
    pub g_norm: f32,
    /// Render→capture delay in ms (device buffers + acoustic path), 10ms envelope resolution.
    pub delay_ms: u32,
    /// Volume (dB) at measure time; None on desktop.
    pub cal_vol_db: Option<f32>,
    pub route_id: String,
}

/// The voice profile: how the user's natural speech lands on this MIC.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceProfile {
    /// Fixed mic gain toward the engine's TX target (replaces the chasing AGC).
    pub mic_gain: f32,
    /// Room noise floor (mean |sample| per frame).
    pub floor: f32,
    pub mic_id: String,
}

/// A finished measurement, handed to the app drain (which owns settings).
#[derive(Debug, Clone, PartialEq)]
pub enum CalResult {
    Echo(EchoProfile),
    Voice(VoiceProfile),
}

static RESULT: Mutex<Option<CalResult>> = Mutex::new(None);

pub fn take_result() -> Option<CalResult> {
    RESULT.lock().unwrap().take()
}

pub struct CalHandle {
    stop: Arc<AtomicBool>,
}

impl CalHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for CalHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Decode the PHPRMPT1 asset to 10ms PCM frames. `None` on a corrupt asset (a build problem, not a runtime one — but never a panic).
fn decode_prompt() -> Option<Vec<Vec<i16>>> {
    let bytes = PROMPT_ASSET;
    if bytes.len() < 8 || &bytes[..8] != b"PHPRMPT1" {
        return None;
    }
    let mut dec = opus::Decoder::new(48_000, opus::Channels::Mono).ok()?;
    let mut frames = Vec::new();
    let mut p = 8usize;
    let mut pcm = vec![0i16; FRAME_SAMPLES];
    while p + 2 <= bytes.len() {
        let len = u16::from_le_bytes([bytes[p], bytes[p + 1]]) as usize;
        p += 2;
        if p + len > bytes.len() {
            return None;
        }
        let n = dec.decode(&bytes[p..p + len], &mut pcm, false).ok()?;
        // -9 dB (×363/1024 ≈ 0.354): the master is normalized to full scale, which at a music-listening media volume is DEAFENING (field 2026-09-02) — real call audio never runs that hot. Measurement-neutral: the correlation's render reference and the acoustic emission scale together, so delay and coupling g come out identical; only the user's ears notice.
        frames.push(pcm[..n].iter().map(|&s| ((s as i32 * 363) >> 10) as i16).collect());
        p += len;
    }
    Some(frames)
}

/// Mean |sample| of a frame — the envelope unit every measurement runs in (matches the engine's level math).
pub(crate) fn env(frame: &[i16]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    frame.iter().map(|s| s.unsigned_abs() as u64).sum::<u64>() as f32 / frame.len() as f32
}

/// Envelope cross-correlation: find the lag (in frames) where `cap` best matches `play`, scanning 0..=max_lag. Returns (lag, gain) — gain = Σcap·play / Σplay² at the peak (the least-squares amplitude ratio, robust against mic noise). Envelope-domain (10ms bins), exactly the resolution the frame-quantized duck needs.
pub(crate) fn envelope_xcorr(play: &[f32], cap: &[f32], max_lag: usize) -> Option<(usize, f32)> {
    if play.is_empty() || cap.len() < play.len() {
        return None;
    }
    let play_energy: f64 = play.iter().map(|&x| (x as f64) * (x as f64)).sum();
    if play_energy <= 0.0 {
        return None;
    }
    let mut best = (0usize, f64::MIN);
    for lag in 0..=max_lag.min(cap.len().saturating_sub(play.len())) {
        let dot: f64 = play
            .iter()
            .zip(&cap[lag..lag + play.len()])
            .map(|(&a, &b)| (a as f64) * (b as f64))
            .sum();
        if dot > best.1 {
            best = (lag, dot);
        }
    }
    let gain = (best.1 / play_energy).max(0.0) as f32;
    Some((best.0, gain))
}

/// Trimmed median of voiced frame envelopes: frames above `floor × 4` are voice; drop the top+bottom eighth (onset hesitancy / trail-off), take the median of the rest. None = the user never spoke above the floor.
pub(crate) fn voiced_level(envs: &[f32], floor: f32) -> Option<f32> {
    let mut voiced: Vec<f32> = envs.iter().copied().filter(|&e| e > floor * 4.0 + 40.0).collect();
    if voiced.len() < 20 {
        return None; // under 200ms of speech — didn't catch it
    }
    voiced.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let trim = voiced.len() / 8;
    let mid = &voiced[trim..voiced.len() - trim];
    Some(mid[mid.len() / 2])
}

const PACE_TARGET: usize = 6; // frames of render queue — track the DAC, don't race it (playback.rs's constant)
const TAIL_FRAMES: usize = 100; // 1s of post-prompt silence = noise-floor sample
const REPEAT_FRAMES: usize = 700; // 7s of capture FROM VOICE ONSET (the sentence + settle)
const ONSET_WAIT_FRAMES: usize = 800; // 8s grace to START speaking after the beat flips — the fixed window used to open at the flip and a slow start burned it (macbook field 2026-09-03, "Didn't catch that")
const MAX_LAG_FRAMES: usize = 100; // 1s of correlation scan — way past any real render→capture path

/// Wait for the platform to resolve an identity string (route/mic). The cpal/Kotlin device sniff lands ASYNCHRONOUSLY after session start, so the first calibration of a session read "" as its baseline and the settled name at verdict — a phantom "changed mid-run" void (macbook field 2026-09-03, echo attempt 1). Not a UI timer: this is measurement warmup on the worker thread, bounded at 2s.
fn settle_identity(read: impl Fn() -> String, stop: &AtomicBool) -> String {
    for _ in 0..200 {
        let v = read();
        if !v.is_empty() || stop.load(Ordering::Relaxed) {
            return v;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    read()
}

/// Capture until the user's voice STARTS (env climbs past the live floor estimate), up to the grace bound. Returns (collected frames, onset?, stopped?). The onset needs ≥500ms of baseline first so the example's reverb tail can't fake it.
fn wait_for_onset(stop: &AtomicBool) -> (Vec<f32>, bool, bool) {
    use std::time::Duration;
    let mut envs: Vec<f32> = Vec::new();
    while envs.len() < ONSET_WAIT_FRAMES && !stop.load(Ordering::Relaxed) {
        for f in crate::platform::audio::captured_frames() {
            let e = env(&f);
            envs.push(e);
            if envs.len() >= 50 {
                let floor = quietest_run(&envs, 30);
                if e > floor * 4.0 + 40.0 {
                    return (envs, true, false);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    (envs, false, stop.load(Ordering::Relaxed))
}

fn begin(phase0: CalPhase) -> Option<(Vec<Vec<i16>>, Arc<AtomicBool>)> {
    if crate::platform::audio::is_active() {
        crate::log("CAL: audio busy (call active) — refused");
        return None;
    }
    let frames = decode_prompt()?;
    if !crate::platform::audio::start() {
        crate::platform::audio::stop();
        return None;
    }
    set_phase(phase0);
    Some((frames, Arc::new(AtomicBool::new(false))))
}

fn finish(verdict: Option<CalResult>) {
    crate::platform::audio::stop();
    match verdict {
        Some(r) => {
            *RESULT.lock().unwrap() = Some(r);
            set_phase(CalPhase::Done);
        }
        None => {
            crate::log("CAL: gates failed — nothing stored, ask to retry");
            set_phase(CalPhase::Failed);
        }
    }
}

/// Play the prompt while collecting mic envelopes; returns (captured envelopes, stopped?).
fn play_and_capture(prompt: &[Vec<i16>], stop: &AtomicBool, capture: bool) -> (Vec<f32>, bool) {
    use std::time::Duration;
    let _ = crate::platform::audio::captured_frames();
    let mut cap: Vec<f32> = Vec::new();
    let mut queued = 0usize;
    while (queued < prompt.len() || crate::platform::audio::playback_depth() > 0)
        && !stop.load(Ordering::Relaxed)
    {
        while queued < prompt.len() && crate::platform::audio::playback_depth() < PACE_TARGET {
            crate::platform::audio::queue_playback(prompt[queued].clone());
            queued += 1;
        }
        if capture {
            for f in crate::platform::audio::captured_frames() {
                cap.push(env(&f));
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    (cap, stop.load(Ordering::Relaxed))
}

/// Collect `n` mic-envelope frames.
fn capture_frames(n: usize, stop: &AtomicBool) -> (Vec<f32>, bool) {
    use std::time::Duration;
    let mut cap: Vec<f32> = Vec::with_capacity(n);
    while cap.len() < n && !stop.load(Ordering::Relaxed) {
        for f in crate::platform::audio::captured_frames() {
            cap.push(env(&f));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    (cap, stop.load(Ordering::Relaxed))
}

/// ECHO CHECK — the user stays quiet; the prompt is the probe.
pub fn start_echo() -> Option<CalHandle> {
    let (prompt, stop) = begin(CalPhase::EchoListen)?;
    let flag = stop.clone();
    std::thread::Builder::new()
        .name("audio-cal-echo".into())
        .spawn(move || {
            let route_id = settle_identity(crate::platform::audio::route_id, &flag);
            let cal_vol_db = crate::platform::audio::current_volume_db();
            let play_env: Vec<f32> = prompt.iter().map(|f| env(f)).collect();
            let (mut cap, stopped) = play_and_capture(&prompt, &flag, true);
            if stopped {
                finish(None);
                return;
            }
            // Tail: 1s of room after the prompt drains — the correlation scan needs the lag headroom and the tail samples the floor.
            let (tail, stopped) = capture_frames(TAIL_FRAMES, &flag);
            cap.extend_from_slice(&tail);
            if stopped {
                finish(None);
                return;
            }
            // Stability gates: volume knob or route swap mid-run voids the measurement.
            if let (Some(a), Some(b)) = (cal_vol_db, crate::platform::audio::current_volume_db()) {
                if (a - b).abs() > 0.5 {
                    crate::log("CAL: volume moved mid-run — measurement void");
                    finish(None);
                    return;
                }
            }
            if !route_id.is_empty() && crate::platform::audio::route_id() != route_id {
                crate::log("CAL: output route changed mid-run — measurement void");
                finish(None);
                return;
            }
            let Some((lag, g)) = envelope_xcorr(&play_env, &cap, MAX_LAG_FRAMES) else {
                crate::log("CAL: echo — correlation had nothing to bite on (empty/short capture) — didn't catch it");
                finish(None);
                return;
            };
            // Sanity: a real coupling needs a plausible lag; lag 0 with real g = smeared correlation (echoey hall) — retry, don't store garbage. lag 0 with g≈0 is a headset: legal.
            if g > 0.01 && lag == 0 {
                crate::logf!(
                    "CAL: echo — g {} at lag 0 = smeared correlation (echoey room?) — retry, not storing",
                    format!("{g:.4}")
                );
                finish(None);
                return;
            }
            if !g.is_finite() {
                crate::log("CAL: echo — non-finite coupling — retry, not storing");
                finish(None);
                return;
            }
            let g_norm = match cal_vol_db {
                Some(db) => g / 10f32.powf(db / 20.0),
                None => g,
            };
            crate::logf!(
                "CAL: echo — route \"{}\" g {} delay {}ms vol {}",
                route_id,
                format!("{g_norm:.4}"),
                lag * 10,
                format!("{cal_vol_db:?}")
            );
            finish(Some(CalResult::Echo(EchoProfile {
                g_norm,
                delay_ms: (lag as u32) * 10,
                cal_vol_db,
                route_id,
            })));
        })
        .ok()?;
    Some(CalHandle { stop })
}

/// VOICE CHECK — the example plays first (listen), then the mic window opens (repeat).
pub fn start_voice() -> Option<CalHandle> {
    let (prompt, stop) = begin(CalPhase::VoiceExample)?;
    let flag = stop.clone();
    std::thread::Builder::new()
        .name("audio-cal-voice".into())
        .spawn(move || {
            let mic_id = settle_identity(crate::platform::audio::mic_id, &flag);
            // Example pass — no capture needed; the user is listening.
            let (_, stopped) = play_and_capture(&prompt, &flag, false);
            if stopped {
                finish(None);
                return;
            }
            set_phase(CalPhase::VoiceRepeat);
            // ONSET-GATED window: the edge is the user's voice starting, not the phase flip — a slow start (or a phase flip they hadn't seen yet) no longer burns the window.
            let (pre, onset, stopped) = wait_for_onset(&flag);
            if stopped {
                finish(None);
                return;
            }
            if !onset {
                let floor = quietest_run(&pre, 30);
                crate::logf!(
                    "CAL: voice — heard nothing above the floor for 8s (floor {}) — didn't catch it",
                    format!("{floor:.0}")
                );
                finish(None);
                return;
            }
            let (rep, stopped) = capture_frames(REPEAT_FRAMES, &flag);
            if stopped {
                finish(None);
                return;
            }
            if !mic_id.is_empty() && crate::platform::audio::mic_id() != mic_id {
                crate::log("CAL: mic changed mid-run — measurement void");
                finish(None);
                return;
            }
            // Analyse the whole take (pre-onset quiet + speech): the wait's frames ARE the floor sample.
            let mut all = pre;
            all.extend_from_slice(&rep);
            let floor = quietest_run(&all, 30);
            let Some(talk) = voiced_level(&all, floor) else {
                crate::logf!(
                    "CAL: voice — under 200ms of speech above the floor (floor {}) — didn't catch it",
                    format!("{floor:.0}")
                );
                finish(None);
                return;
            };
            if talk <= floor * 4.0 {
                crate::logf!(
                    "CAL: voice — level {} within 4x of floor {} — spoke too soft or the room is too loud",
                    format!("{talk:.0}"),
                    format!("{floor:.0}")
                );
                finish(None);
                return;
            }
            let mic_gain = (4000.0 / talk.max(1.0)).clamp(0.125, 8.0); // TX_TARGET_LEVEL / talk, within the engine's GAIN bounds
            crate::logf!(
                "CAL: voice — mic \"{}\" gain {} floor {}",
                mic_id,
                format!("{mic_gain:.2}"),
                format!("{floor:.0}")
            );
            finish(Some(CalResult::Voice(VoiceProfile { mic_gain, floor, mic_id })));
        })
        .ok()?;
    Some(CalHandle { stop })
}

/// Reset the phase to Idle (result consumed / page left).
pub fn ack_phase() {
    set_phase(CalPhase::Idle);
}

fn mean(v: &[f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.iter().sum::<f32>() / v.len() as f32
}

/// The quietest `n`-frame run's mean — the pre/mid-speech gap, a floor sample robust to the user talking thru most of the window.
pub(crate) fn quietest_run(envs: &[f32], n: usize) -> f32 {
    if envs.len() < n {
        return mean(envs);
    }
    let mut best = f32::MAX;
    let mut sum: f32 = envs[..n].iter().sum();
    best = best.min(sum / n as f32);
    for i in n..envs.len() {
        sum += envs[i] - envs[i - n];
        best = best.min(sum / n as f32);
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_asset_decodes() {
        let frames = decode_prompt().expect("embedded prompt must decode");
        assert!(frames.len() > 300 && frames.len() < 500, "got {} frames", frames.len());
        assert!(frames.iter().all(|f| f.len() == FRAME_SAMPLES));
        let peak = frames.iter().map(|f| env(f) as u32).max().unwrap();
        assert!(peak > 500, "prompt peak envelope {peak} — asset silent?");
    }

    #[test]
    fn xcorr_recovers_lag_and_gain() {
        let play: Vec<f32> = (0..200)
            .map(|i| if (i / 20) % 2 == 0 { 1000.0 + (i % 20) as f32 * 30.0 } else { 50.0 })
            .collect();
        let mut cap = vec![25.0f32; 17];
        cap.extend(play.iter().map(|&x| x * 0.31 + 20.0));
        cap.extend(std::iter::repeat(25.0).take(50));
        let (lag, g) = envelope_xcorr(&play, &cap, 100).unwrap();
        assert_eq!(lag, 17);
        assert!((g - 0.31).abs() < 0.05, "gain {g}");
    }

    #[test]
    fn voiced_median_ignores_onset_and_silence() {
        let floor = 30.0;
        let mut envs = vec![28.0f32; 100];
        envs.extend((0..10).map(|i| 200.0 + i as f32 * 300.0));
        envs.extend(std::iter::repeat(3000.0).take(150));
        envs.extend((0..10).map(|i| 3000.0 - i as f32 * 280.0));
        envs.extend(std::iter::repeat(28.0).take(100));
        let talk = voiced_level(&envs, floor).unwrap();
        assert!((talk - 3000.0).abs() < 200.0, "talk {talk}");
        assert!(voiced_level(&vec![25.0; 400], floor).is_none());
    }

    #[test]
    fn quietest_run_finds_the_gap() {
        let mut envs = vec![3000.0f32; 100];
        envs.extend(std::iter::repeat(20.0).take(40));
        envs.extend(std::iter::repeat(2500.0).take(100));
        assert!(quietest_run(&envs, 30) < 30.0);
    }
}
