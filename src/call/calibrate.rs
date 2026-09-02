//! Per-route audio calibration (the approved plan, 2026-09-02) — measure once, then leave the mic alone.
//!
//! The ritual: the device PLAYS Nick's recorded prompt ("the quick brown fox jumped over the lazy dogs") while the user just listens — that playback IS the measurement probe: cross-correlating what the mic heard against what we played yields the render→capture DELAY and the speaker→mic COUPLING gain `g`, and the tail silence samples the room noise floor. Then the screen asks the user to REPEAT the sentence; the trimmed median of their voiced frames sets a FIXED mic gain. One recording serves as both instructions-by-example and probe — no second asset needed.
//!
//! The profile turns the in-call duck from a heuristic into a prediction: expected-echo-at-mic = `g ×` (delay-compensated render level, volume-scaled). Mic ≈ prediction → pure echo, gate hard; mic ≫ prediction → the human, let it thru. No absolute thresholds — the quiet-talker asymmetry (Brittany/Nick field calls) cannot recur.
//!
//! Session ownership mirrors `call/playback.rs`: refuses while a call owns the audio; the worker holds start()/stop() for its whole run. Phases are EDGES on the state machine (playback drained, capture window filled) — no wall-clock pacing beyond the DAC itself.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

/// The embedded prompt: PHPRMPT1 framing (magic + [len u16 LE][opus packet]…), minted by `promptenc` from the recorded master at the call's top rung (48k mono CELT LowDelay @128k, 10ms frames).
const PROMPT_ASSET: &[u8] = include_bytes!("../../assets/cal-prompt.opusp");
const FRAME_SAMPLES: usize = 480; // 10ms @ 48k — matches platform::audio

/// Where a running calibration stands — published for the Audio page to render live. u8-backed so the worker can post lock-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalPhase {
    Idle = 0,
    /// Playing the prompt; user should stay quiet. (The measurement.)
    Listen = 1,
    /// Screen asks the user to repeat the sentence; mic window open.
    Repeat = 2,
    /// Worker finished — result posted (profile stored by the app drain, or a retry verdict).
    Done = 3,
    /// A sanity gate failed — nothing stored; the page says try again.
    Failed = 4,
}

static PHASE: AtomicU8 = AtomicU8::new(0);

pub fn phase() -> CalPhase {
    match PHASE.load(Ordering::Relaxed) {
        1 => CalPhase::Listen,
        2 => CalPhase::Repeat,
        3 => CalPhase::Done,
        4 => CalPhase::Failed,
        _ => CalPhase::Idle,
    }
}

/// A completed measurement, handed to the app drain (which owns settings) for storage under `audio.cal.<route_id>`.
#[derive(Debug, Clone, PartialEq)]
pub struct CalProfile {
    /// Fixed mic gain toward the engine's TX target (replaces the chasing AGC on calibrated routes).
    pub mic_gain: f32,
    /// Room noise floor (mean |sample| per frame).
    pub floor: f32,
    /// Speaker→mic coupling: captured level ÷ rendered level at the correlation peak, normalized to the calibration volume (vol scaling re-applies at call time).
    pub g_norm: f32,
    /// Render→capture delay in ms (device buffer + acoustic path) at 10ms envelope resolution.
    pub delay_ms: u32,
    /// The volume (dB) the profile was measured at; None on desktop (no volume API).
    pub cal_vol_db: Option<f32>,
    /// The route this profile belongs to.
    pub route_id: String,
}

/// The one in-flight result slot (worker → app tick). Mutex over a channel: exactly one calibration runs at a time.
static RESULT: Mutex<Option<CalProfile>> = Mutex::new(None);

pub fn take_result() -> Option<CalProfile> {
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
        frames.push(pcm[..n].to_vec());
        p += len;
    }
    Some(frames)
}

/// Mean |sample| of a frame — the envelope unit every measurement below runs in (matches the engine's level math).
fn env(frame: &[i16]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    frame.iter().map(|s| s.unsigned_abs() as u64).sum::<u64>() as f32 / frame.len() as f32
}

/// Envelope cross-correlation: find the lag (in frames) where `cap` best matches `play`, scanning 0..=max_lag. Returns (lag, gain) — gain = Σcap·play / Σplay² at the peak (the least-squares amplitude ratio, robust against mic noise). Envelope-domain (10ms bins), which is exactly the resolution the frame-quantized duck needs.
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

/// Run a calibration for the CURRENT route. `None` if a call owns the audio session or the prompt asset is unreadable. The worker owns the session for its whole run; the app tick polls `phase()` for the live screen and `take_result()` for the profile to store.
pub fn start() -> Option<CalHandle> {
    if crate::platform::audio::is_active() {
        crate::log("CAL: audio busy (call active) — refused");
        return None;
    }
    let frames = decode_prompt()?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    if !crate::platform::audio::start() {
        crate::platform::audio::stop();
        return None;
    }
    PHASE.store(CalPhase::Listen as u8, Ordering::Relaxed);
    let spawned = std::thread::Builder::new()
        .name("audio-cal".into())
        .spawn(move || {
            let verdict = run(frames, &flag);
            crate::platform::audio::stop();
            match verdict {
                Some(profile) => {
                    crate::logf!(
                        "CAL: profile — route \"{}\" gain {} floor {} g {} delay {}ms vol {}",
                        profile.route_id,
                        format!("{:.2}", profile.mic_gain),
                        format!("{:.0}", profile.floor),
                        format!("{:.4}", profile.g_norm),
                        profile.delay_ms,
                        format!("{:?}", profile.cal_vol_db)
                    );
                    *RESULT.lock().unwrap() = Some(profile);
                    PHASE.store(CalPhase::Done as u8, Ordering::Relaxed);
                }
                None => {
                    crate::log("CAL: sanity gates failed — nothing stored, ask to retry");
                    PHASE.store(CalPhase::Failed as u8, Ordering::Relaxed);
                }
            }
        })
        .is_ok();
    if !spawned {
        crate::platform::audio::stop();
        PHASE.store(CalPhase::Idle as u8, Ordering::Relaxed);
        return None;
    }
    Some(CalHandle { stop })
}

/// Reset the phase to Idle (the page leaves / the result was consumed).
pub fn ack_phase() {
    PHASE.store(CalPhase::Idle as u8, Ordering::Relaxed);
}

const PACE_TARGET: usize = 6; // frames of render queue — track the DAC, don't race it (playback.rs's constant)
const TAIL_FRAMES: usize = 100; // 1s of post-prompt silence = noise-floor sample
const REPEAT_FRAMES: usize = 700; // 7s window to say the sentence
const MAX_LAG_FRAMES: usize = 100; // 1s of correlation scan — way past any real render→capture path

fn run(prompt: Vec<Vec<i16>>, stop: &AtomicBool) -> Option<CalProfile> {
    use std::time::Duration;
    let route_id = crate::platform::audio::route_id();
    let cal_vol_db = crate::platform::audio::current_volume_db();
    let route_at_start = route_id.clone();
    let play_env: Vec<f32> = prompt.iter().map(|f| env(f)).collect();

    // ---- Phase A: play the prompt, capture simultaneously (the user listens, quiet). ----
    let _ = crate::platform::audio::captured_frames(); // drop anything stale
    let mut cap_env: Vec<f32> = Vec::with_capacity(play_env.len() + MAX_LAG_FRAMES + TAIL_FRAMES);
    let mut queued = 0usize;
    while (queued < prompt.len() || cap_env.len() < play_env.len() + MAX_LAG_FRAMES) && !stop.load(Ordering::Relaxed) {
        while queued < prompt.len() && crate::platform::audio::playback_depth() < PACE_TARGET {
            crate::platform::audio::queue_playback(prompt[queued].clone());
            queued += 1;
        }
        for f in crate::platform::audio::captured_frames() {
            cap_env.push(env(&f));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    // ---- Tail: 1s of room after the prompt drains — noise floor #1. ----
    let tail_start = cap_env.len();
    while cap_env.len() < tail_start + TAIL_FRAMES && !stop.load(Ordering::Relaxed) {
        for f in crate::platform::audio::captured_frames() {
            cap_env.push(env(&f));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    if stop.load(Ordering::Relaxed) {
        return None;
    }
    let floor_a = mean(&cap_env[tail_start..]);
    let (lag, g) = envelope_xcorr(&play_env, &cap_env, MAX_LAG_FRAMES)?;

    // ---- Phase B: the user repeats the sentence. Pre-speech gap doubles as floor #2. ----
    PHASE.store(CalPhase::Repeat as u8, Ordering::Relaxed);
    let _ = crate::platform::audio::captured_frames();
    let mut rep_env: Vec<f32> = Vec::with_capacity(REPEAT_FRAMES);
    while rep_env.len() < REPEAT_FRAMES && !stop.load(Ordering::Relaxed) {
        for f in crate::platform::audio::captured_frames() {
            rep_env.push(env(&f));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    if stop.load(Ordering::Relaxed) {
        return None;
    }
    let floor = floor_a.max(quietest_run(&rep_env, 30));
    let talk = voiced_level(&rep_env, floor)?;

    // ---- Stability gates (Nick 2026-09-02: "make sure they aren't fidoodling with buttons — we're listening during the announcement"): a volume change or a route swap MID-RUN invalidates the coupling measurement (g scales with volume; a different device is a different acoustic path). Fail loud, ask to redo — never store a profile measured across a knob twist. ----
    let vol_now = crate::platform::audio::current_volume_db();
    if let (Some(a), Some(b)) = (cal_vol_db, vol_now) {
        if (a - b).abs() > 0.5 {
            crate::logf!("CAL: volume moved mid-run ({} → {} dB) — measurement void", format!("{a:.1}"), format!("{b:.1}"));
            return None;
        }
    }
    if crate::platform::audio::route_id() != route_at_start {
        crate::log("CAL: output route changed mid-run — measurement void");
        return None;
    }

    // ---- Sanity gates (the plan's): a stored profile is a MEASUREMENT, never a guess. ----
    if !(1..=MAX_LAG_FRAMES).contains(&lag) && g > 0.01 {
        // lag 0 with real coupling = the correlation smeared (echoey hall) — retry rather than store garbage. (lag 0 with g≈0 is a headset: fine, falls thru.)
        return None;
    }
    if !g.is_finite() || talk <= floor * 4.0 {
        return None;
    }
    let mic_gain = (4000.0 / talk.max(1.0)).clamp(0.125, 8.0); // TX_TARGET_LEVEL / talk, within the engine's GAIN bounds
    // Volume-normalize g: store at "unit volume" so call-time scales by the current volume delta (Android); desktop stores as-measured.
    let g_norm = match cal_vol_db {
        Some(db) => g / 10f32.powf(db / 20.0),
        None => g,
    };
    Some(CalProfile {
        mic_gain,
        floor,
        g_norm,
        delay_ms: (lag as u32) * 10,
        cal_vol_db,
        route_id,
    })
}

fn mean(v: &[f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.iter().sum::<f32>() / v.len() as f32
}

/// The quietest `n`-frame run's mean — the pre/mid-speech gap, a floor sample robust to the user talking thru most of the window.
fn quietest_run(envs: &[f32], n: usize) -> f32 {
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
        // ~4s of 10ms frames, all full-length.
        assert!(frames.len() > 300 && frames.len() < 500, "got {} frames", frames.len());
        assert!(frames.iter().all(|f| f.len() == FRAME_SAMPLES));
        // The prompt has real audio in it (not silence).
        let peak = frames.iter().map(|f| env(f) as u32).max().unwrap();
        assert!(peak > 500, "prompt peak envelope {peak} — asset silent?");
    }

    #[test]
    fn xcorr_recovers_lag_and_gain() {
        // Synthetic speech-ish envelope: bursts and gaps.
        let play: Vec<f32> = (0..200)
            .map(|i| if (i / 20) % 2 == 0 { 1000.0 + (i % 20) as f32 * 30.0 } else { 50.0 })
            .collect();
        // Captured = delayed 17 frames, attenuated ×0.31, plus noise floor.
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
        // 1s silence, then speech ramping in, steady, trailing off.
        let mut envs = vec![28.0f32; 100];
        envs.extend((0..10).map(|i| 200.0 + i as f32 * 300.0)); // onset ramp
        envs.extend(std::iter::repeat(3000.0).take(150)); // steady voice
        envs.extend((0..10).map(|i| 3000.0 - i as f32 * 280.0)); // trail
        envs.extend(std::iter::repeat(28.0).take(100));
        let talk = voiced_level(&envs, floor).unwrap();
        assert!((talk - 3000.0).abs() < 200.0, "talk {talk}");
        // Pure silence → None, never a made-up level.
        assert!(voiced_level(&vec![25.0; 400], floor).is_none());
    }

    #[test]
    fn quietest_run_finds_the_gap() {
        let mut envs = vec![3000.0f32; 100];
        envs.extend(std::iter::repeat(20.0).take(40)); // the gap
        envs.extend(std::iter::repeat(2500.0).take(100));
        assert!(quietest_run(&envs, 30) < 30.0);
    }
}
