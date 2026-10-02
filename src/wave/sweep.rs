//! THE CONNECT SWEEP (Nick 2026-09-29): every wave opens with one second of log sweep, 10 Hz → 20 kHz, played on our own speaker OVER the wave (mixed in, never holding or replacing it), and every wave that was connected ends with the same sweep reversed. It is the connect / disconnect indicator, and the opening one doubles as calibration.
//!
//! On a log sweep every octave takes the same time — 20 kHz / 10 Hz ≈ 11 octaves in a second, ~91 ms each — so the part a phone speaker and an ear actually carry (a few hundred Hz to ~10 kHz) is roughly the middle quarter second; the rest is below the speaker or above the ear.
//! CALIBRATION: our mic hears our own speaker. Matching the capture against the sweep gives the speaker→mic path in true time — the acoustic flight plus whatever the HAL's two timestamps disagree by — and its strength. That is the `a` in the echo arithmetic: our voice, played at the far DAC at our name + their l, reaches their mic `a` later; each side measures its own `a` here.
//! The sweep's time-bandwidth product (~1 s × 20 kHz) is ~43 dB of processing gain, so speech at answer barely dents the peak.

/// The native rate — the grid's.
const RATE: usize = 48_000;
/// One second of sweep.
pub const SWEEP_SAMPLES: usize = RATE;
const F0: f64 = 10.0;
const F1: f64 = 20_000.0;
/// Peak level: 1.5 stops under full scale — the same loudness law as the old probe (full scale at media volume is deafening).
const PEAK: f64 = 11_585.0;
/// How far past the sweep's first rendered sample the mic is searched for it: 250 ms covers any built-in or wired route (Bluetooth's buffering can run longer; a reject says so rather than guessing).
pub const MAX_LAG: usize = RATE / 4;
/// A peak must stand this far above the correlation's median to be a real coupling; under it the route is clean (a headset).
const PSR_MIN: f64 = 6.0;

static UP: std::sync::OnceLock<Vec<i16>> = std::sync::OnceLock::new();

/// The up sweep, built once. Phase φ(t) = 2π·f0·T/ln k · (k^(t/T) − 1): zero at the start (so it begins on a zero crossing), and the total phase trimmed (≪ 0.1 %, inaudible) so the last sample lands on a multiple of π — no click at either end, no fades.
pub fn up() -> &'static [i16] {
    UP.get_or_init(|| {
        let t_total = SWEEP_SAMPLES as f64 / RATE as f64;
        let k = F1 / F0;
        let c = std::f64::consts::TAU * F0 * t_total / k.ln();
        let t_last = (SWEEP_SAMPLES - 1) as f64 / RATE as f64;
        let raw_last = c * (k.powf(t_last / t_total) - 1.0);
        let trim = (raw_last / std::f64::consts::PI).round() * std::f64::consts::PI / raw_last;
        (0..SWEEP_SAMPLES)
            .map(|i| {
                let t = i as f64 / RATE as f64;
                ((c * (k.powf(t / t_total) - 1.0) * trim).sin() * PEAK).round() as i16
            })
            .collect()
    })
}

/// The disconnect sound: the up sweep, time-reversed.
pub fn down() -> Vec<i16> {
    up().iter().rev().copied().collect()
}

/// What the mic heard of our own sweep.
#[derive(Clone, Copy, Debug)]
pub struct Fit {
    /// Samples from the sweep's first rendered sample to where the mic caught it (true time on our grid).
    pub delay: i64,
    /// Coupling: the mic's copy over the emitted sweep (matched-filter peak / template energy).
    pub gain: f32,
    /// Peak over the correlation's median magnitude.
    pub psr: f64,
    pub coupled: bool,
}

/// Find the sweep in `cap` (mic samples, `cap[0]` at grid sample `cap_k0`), rendered from grid sample `render_k`: scan lags 0..=MAX_LAG after the render start. `None` when the capture does not cover the scan.
pub fn fit(cap: &[i16], cap_k0: i64, render_k: i64) -> Option<Fit> {
    let tpl = up();
    let n = tpl.len();
    let first = render_k - cap_k0; // capture index of the render start
    if first < 0 {
        return None;
    }
    let first = first as usize;
    let scan = MAX_LAG.min(cap.len().checked_sub(first + n)?);
    let energy: f64 = tpl.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>().max(1e-9); // WHY/PROOF: a template of silence has no energy to normalise by
    let mut mags: Vec<f64> = Vec::with_capacity(scan + 1);
    let mut best = (0usize, 0f64);
    for lag in 0..=scan {
        let at = first + lag;
        let dot: i64 = cap[at..at + n].iter().zip(tpl).map(|(&c, &r)| c as i64 * r as i64).sum();
        let mag = (dot as f64).abs();
        mags.push(mag);
        if mag > best.1 {
            best = (lag, mag);
        }
    }
    mags.sort_by(|a, b| a.total_cmp(b));
    let median = mags[mags.len() / 2].max(1e-9); // WHY/PROOF: a silent capture's median magnitude is 0, and the ratio divides by it
    let psr = best.1 / median;
    Some(Fit { delay: best.0 as i64, gain: (best.1 / energy) as f32, psr, coupled: psr >= PSR_MIN })
}

/// Run the fit off the engine thread (a quarter second of lags × a second of template is a few hundred ms of CPU) and log what it found.
pub fn finish(cap: Vec<i16>, cap_k0: i64, render_k: i64, route: String) {
    let _ = std::thread::Builder::new().name("sweep-fit".into()).spawn(move || {
        let t0 = std::time::Instant::now();
        let fitted = fit(&cap, cap_k0, render_k);
        match fitted {
            None => crate::log("WAVE: sweep — the capture did not cover the sweep (mic late or muted); no measurement"),
            Some(f) if !f.coupled => crate::logf!("WAVE: sweep — clean route \"{}\": no speaker→mic coupling above the noise (psr {:.1}), fit {} ms", route, f.psr, t0.elapsed().as_millis()),
            Some(f) => crate::logf!(
                "WAVE: sweep — speaker→mic {:.2} ms ({} samples), coupling {:.4}, psr {:.1}, route \"{}\", fit {} ms",
                f.delay as f64 * 1000.0 / RATE as f64,
                f.delay,
                f.gain,
                f.psr,
                route,
                t0.elapsed().as_millis()
            ),
        }
        // The duck's depth follows the measurement (Nick 2026-10-02: a clean route suppresses nothing): zero for a clean route, the coupling itself otherwise; an unmeasurable sweep leaves the route's prior.
        if let Some(f) = fitted {
            crate::platform::audio::set_duck_coupling(if f.coupled { f.gain.max(0.0) } else { 0.0 });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sweep starts on a zero crossing, ends within one step of zero, peaks at the level law, and the down sweep is its exact reverse.
    #[test]
    fn sweep_shape() {
        let u = up();
        assert_eq!(u.len(), SWEEP_SAMPLES);
        assert_eq!(u[0], 0);
        assert!(u[SWEEP_SAMPLES - 1].unsigned_abs() < 12_000);
        assert!(u.iter().map(|v| v.unsigned_abs()).max().unwrap() as f64 <= PEAK + 1.0);
        let d = down();
        assert_eq!(d[0], u[SWEEP_SAMPLES - 1]);
        assert_eq!(d[SWEEP_SAMPLES - 1], u[0]);
    }

    /// A capture holding the sweep 3.5 ms after its render start (quieter, over noise) fits to that delay, coupled; silence reads clean.
    #[test]
    fn finds_the_sweep_in_the_mic() {
        let delay = 168usize; // 3.5 ms
        let cap_k0 = 1_000_000i64;
        let render_k = cap_k0 + 480; // the render starts 10 ms into the capture
        let len = 480 + SWEEP_SAMPLES + MAX_LAG + 480;
        let mut seed = 12345u32;
        let mut cap: Vec<i16> = (0..len)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                ((seed >> 16) as i16) >> 6
            })
            .collect();
        for (i, &s) in up().iter().enumerate() {
            let at = 480 + delay + i;
            cap[at] = cap[at].saturating_add(s / 8);
        }
        let f = fit(&cap, cap_k0, render_k).unwrap();
        assert!(f.coupled, "psr {}", f.psr);
        assert_eq!(f.delay, delay as i64);
        assert!((f.gain - 0.125).abs() < 0.02, "gain {}", f.gain);
        let quiet = vec![0i16; len];
        assert!(!fit(&quiet, cap_k0, render_k).unwrap().coupled);
    }
}
