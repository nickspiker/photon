//! Calibration substrate — the profile types, the learned-result mailbox, and the envelope helpers shared by the in-wave learner ([`super::learn`]) and the v-chirp connect probe ([`super::vchirp`]).
//!
//! The Wave calibration ritual that used to live here (echo check + voice check, the Settings page ceremony) is GONE (2026-09-07): every wave now opens with the v-chirp probe on the live route, so per-route coupling is measured at every connect instead of once per ceremony. Voice profiles still accumulate from the learner's in-wave evidence.

use std::sync::Mutex;

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

/// The voice profile: how the user's natural speech lands on this MIC — since the level plan, the RAW voiced mean |sample| that sets the fixed TX makeup, keyed by route + input so a headset's mic never pollutes the earpiece's number.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceProfile {
    /// Measured raw voiced mean |sample| on this input (pre-makeup).
    pub voiced: f32,
    /// Room noise floor (mean |sample| per frame, raw).
    pub floor: f32,
    pub mic_id: String,
}

/// A finished measurement, handed to the app drain (which owns settings).
#[derive(Debug, Clone, PartialEq)]
pub enum CalResult {
    Echo(EchoProfile),
    Voice(VoiceProfile),
}

/// A measured profile with the evidence weight the blend needs — posted by the engine teardown (learner), a mid-wave route swap, or the v-chirp fit thread.
pub struct LearnedResult {
    pub result: CalResult,
    /// Sample weight: accepted estimator windows (echo) or seconds of voiced speech (voice).
    pub windows: u32,
    /// Solid confidence tier, re-asserted here so the drain can trust it without reaching into the learner.
    pub solid: bool,
}

static LEARNED: Mutex<Vec<LearnedResult>> = Mutex::new(Vec::new());

/// The UI wake for posted results. The event loop is event-driven — a post that doesn't wake it persists only when something ELSE stirs the loop (the "hang on sat forever" law, field 2026-09-02). Registered once by the app; first registration wins.
static WAKE: std::sync::OnceLock<Box<dyn Fn() + Send + Sync>> = std::sync::OnceLock::new();

pub fn register_wake(f: impl Fn() + Send + Sync + 'static) {
    let _ = WAKE.set(Box::new(f));
}

/// Post measured profiles; fires the UI wake so the drain stores them now, not at the next unrelated event.
pub fn post_learned(items: Vec<LearnedResult>) {
    if items.is_empty() {
        return;
    }
    LEARNED.lock().unwrap().extend(items);
    if let Some(w) = WAKE.get() {
        w();
    }
}

pub fn take_learned() -> Vec<LearnedResult> {
    std::mem::take(&mut *LEARNED.lock().unwrap())
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
    fn quietest_run_finds_the_gap() {
        let mut envs = vec![3000.0f32; 100];
        envs.extend(std::iter::repeat(20.0).take(40));
        envs.extend(std::iter::repeat(2500.0).take(100));
        assert!(quietest_run(&envs, 30) < 30.0);
    }
}
