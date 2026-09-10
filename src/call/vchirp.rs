//! The V-chirp connect probe (Nick 2026-09-07, lengthened to 1s 2026-09-08): every call opens with the SAME one-second sound — an up log sweep and its time-reversal overlaid — played thru the live call output path and matched-filtered against the mic, so both sides measure their own speaker→mic coupling `g` and render→capture delay BEFORE any voice connects. This replaces the Wave calibration ritual: first-call-zero-echo is now a property of every call, not a ceremony the user must remember to run per route.
//!
//! Why a V (up + down overlaid), not a single sweep or noise:
//! - **Delay-Doppler split = clock-skew meter.** A render-vs-capture sample-rate mismatch shifts a chirp's matched-filter peak, with OPPOSITE sign for opposite sweep directions (the radar V-chirp trick). Mean of the two lags = true delay with the skew cancelled; difference = the skew itself — the same physics the jitter buffer's splice counters only see indirectly.
//! - **Self-validation.** Two independent estimates of the same path must agree on lag and gain; disagreement = corrupted measurement (speech, movement), rejected instead of stored.
//! - **Robust to "hello".** ~1s × ~19.8kHz of swept bandwidth ≈ 43dB of processing gain — near speech at answer is uncorrelated with the sweep and barely dents the peak, exactly where the learner's envelope statistics are weakest.
//! - Sweeps beat a fixed-seed noise burst (≈MLS) on our two field realities: speaker nonlinearity (distortion products land away from a sweep's linear peak; they smear an MLS everywhere) and clock drift (shifts a sweep's peak; shreds MLS alignment).
//!
//! Template spec (Nick's calls, 2026-09-07): 200Hz → 20kHz log sweep, no fade shaping — the device's own frequency taper rounds the ends, and the raw sweep doubles as a loop frequency-response probe later. The 200Hz end starts AT a zero crossing (phase 0), and the total phase is trimmed to a whole number of cycles so BOTH ends of both legs sit on zeros. The down leg is the exact time-reversal of the up leg — one stored waveform serves as both matched-filter references.
//!
//! Units: `g` is fitted in the ENVELOPE domain (mean |sample| per 10ms bin, median per-bin ratio at the matched delay) — the learner/duck's unit, capturing total leak including early reverb, where the matched filter's coherent gain would read only the direct path and under-duck (the one failure the whole echo design exists to prevent). The matched filter contributes what envelopes can't: sample-precise delay, the skew split, and the corruption gates.

pub const SAMPLE_RATE: usize = 48_000;
const FRAME_SAMPLES: usize = crate::platform::audio::FRAME_SAMPLES;
/// Sweep length: HALF a second (Nick 2026-09-10: "same length but the duration half"), padded to the same one-second emission by a quarter second of silence at each end — the leading silence hands the fit a clean room-floor pre-roll, the trailing one gives the echo tail room, and the shorter sweep halves the anchor drift a short-running capture clock (Nick's phone: 175-193 fps) accumulates across the measurement. Processing gain drops 3 dB from the 1 s sweep; the V's self-validation stands.
pub const PAD_SAMPLES: usize = SAMPLE_RATE / 4;
/// Sweep length: 1s (Nick 2026-09-08 — only the middle of the 200Hz→20kHz span is audible thru a phone transducer, so the perceived sound is well under the full second). 4× the original 250ms: +6dB processing gain (TB ≈ 19.8k ≈ 43dB), 100 envelope bins for the g median instead of 25, and far better leg-agreement statistics — both first field calls rejected on legs disagreeing at 250ms.
pub const CHIRP_SAMPLES: usize = SAMPLE_RATE / 2;
const F0: f64 = 200.0;
const F1: f64 = 20_000.0;
/// Peak of the summed legs: -9dB FS — the same loudness law as the old ritual prompt (full scale at media volume is DEAFENING, field 2026-09-02); measurement-neutral since g is a ratio.
const PEAK_TARGET: f64 = 11_585.0;
/// Delay scan: one second of capture positions — the leading pad (250 ms) plus 750 ms of render→capture path, generously beyond any wired/builtin route (bt scans live in the learner, which refines delay in-call anyway).
pub const MAX_LAG_SAMPLES: usize = SAMPLE_RATE;
/// Post-scan tail: 100ms of room after the last scannable echo position.
const TAIL_SAMPLES: usize = SAMPLE_RATE / 10;
/// The mic capture the fit wants: chirp + scan window + tail — 1.6 s, the same window the one-second sweep used.
pub const CAPTURE_SAMPLES: usize = CHIRP_SAMPLES + MAX_LAG_SAMPLES + TAIL_SAMPLES;
/// Rough spectral bands over the sweep's log-frequency axis (Nick 2026-09-10: "device specific latency, rough spectral"): edges 200 / 500 / 1.2k / 3k / 8k / 20k Hz.
pub const SPECTRAL_BAND_EDGES_HZ: [f64; 6] = [200.0, 500.0, 1200.0, 3000.0, 8000.0, 20000.0];
/// Envelope bin width for the g fit — the learner/duck's 10ms grid.
const BIN: usize = FRAME_SAMPLES;
/// A template envelope bin below this can't excite a measurable echo ratio (mirrors learn::FAR_ACT's intent at the template's own scale).
const TPL_ACT: f32 = 500.0;
/// Peak-to-noise gate: the matched peak must stand this far above the correlation's median |magnitude| to count as a real coupling; below it the route is CLEAN (headset — legal, g ≈ 0).
const PSR_MIN: f64 = 6.0;
/// Leg agreement gates: gains within 4× (same path, generous for room modes), lags within 20ms (a real clock skew over 1s is a few samples; bigger = resampler bug, diagnose don't store).
const LEG_GAIN_RATIO_MAX: f32 = 4.0;
const LEG_LAG_SPLIT_MAX: i64 = (SAMPLE_RATE / 50) as i64;

/// (summed waveform, per-leg emitted amplitude) — the leg scale is PEAK_TARGET/peak(sum), NOT PEAK_TARGET/2: the legs go coherent near the frequency crossing, so the sum's peak sits between 1× and 2× a leg.
static TEMPLATE: std::sync::OnceLock<(Vec<i16>, f64)> = std::sync::OnceLock::new();

fn built() -> &'static (Vec<i16>, f64) {
    TEMPLATE.get_or_init(|| {
        let up = up_leg();
        let n = up.len();
        let mut sum = vec![0f64; n];
        for i in 0..n {
            sum[i] = up[i] + up[n - 1 - i];
        }
        let peak = sum.iter().fold(0f64, |a, &v| a.max(v.abs())).max(1e-9);
        let leg_scale = PEAK_TARGET / peak;
        (sum.iter().map(|&v| (v * leg_scale) as i16).collect(), leg_scale)
    })
}

/// The summed V waveform (up + reversed-up), built once. Both legs start and end on zero crossings by construction — no fades, per the spec.
pub fn template() -> &'static [i16] {
    &built().0
}

/// The up leg in f64 — the matched-filter reference (the down reference is this reversed).
fn up_leg() -> Vec<f64> {
    let t_total = CHIRP_SAMPLES as f64 / SAMPLE_RATE as f64;
    let k = F1 / F0;
    // Log-sweep phase: φ(t) = 2π·f0·T/ln k · (k^(t/T) − 1). The 200Hz end is zero by φ(0)=0; trim total phase (≤0.05%, inaudible) so the LAST SAMPLE's phase is a multiple of π — at 20kHz the phase steps ~0.42 cycles per sample, so without the trim the final sample (and thus the SUM's first sample, which carries the reversed leg's tail) lands anywhere in the cycle.
    let t_last = (CHIRP_SAMPLES - 1) as f64 / SAMPLE_RATE as f64;
    let raw_last = std::f64::consts::TAU * F0 * t_total / k.ln() * (k.powf(t_last / t_total) - 1.0);
    let trim = (raw_last / std::f64::consts::PI).round() * std::f64::consts::PI / raw_last;
    (0..CHIRP_SAMPLES)
        .map(|i| {
            let t = i as f64 / SAMPLE_RATE as f64;
            let phase = std::f64::consts::TAU * F0 * t_total / k.ln() * (k.powf(t / t_total) - 1.0) * trim;
            phase.sin()
        })
        .collect()
}

/// The probe as 5 ms frames for `queue_playback`: a quarter second of silence, the half-second V, a quarter second of silence — one second emitted, UNPADDED in level like the old ritual prompts (it measures the path, so it must not sit under the wave's 4-stop pad).
pub fn frames() -> Vec<Vec<i16>> {
    let mut out: Vec<i16> = Vec::with_capacity(PAD_SAMPLES * 2 + CHIRP_SAMPLES);
    out.resize(PAD_SAMPLES, 0);
    out.extend_from_slice(template());
    out.resize(PAD_SAMPLES * 2 + CHIRP_SAMPLES, 0);
    out.chunks(FRAME_SAMPLES).map(|c| c.to_vec()).collect()
}

/// The sweep's per-band coupling from the per-bin envelope ratios (bin j of the log sweep sits at 200·100^(j/N) Hz): the median ratio inside each SPECTRAL_BAND_EDGES_HZ band, NaN-free (an empty band reads 0).
pub fn spectral_bands(ratios_by_bin: &[(usize, f32)]) -> [f32; 5] {
    let n_bins = (CHIRP_SAMPLES / BIN).max(1) as f64;
    let mut bands = [0f32; 5];
    for b in 0..5 {
        let (lo, hi) = (SPECTRAL_BAND_EDGES_HZ[b], SPECTRAL_BAND_EDGES_HZ[b + 1]);
        let mut v: Vec<f32> = ratios_by_bin
            .iter()
            .filter(|(j, _)| {
                let f = F0 * (F1 / F0).powf(*j as f64 / n_bins);
                f >= lo && f < hi
            })
            .map(|(_, r)| *r)
            .collect();
        if v.is_empty() {
            continue;
        }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        bands[b] = v[v.len() / 2];
    }
    bands
}

/// A finished probe fit. `delay_samples` is the echo's offset WITHIN the capture buffer (the caller anchors it to the render timeline); `skew_samples` = up-leg lag − down-leg lag, the clock-skew diagnostic.
#[derive(Debug, Clone)]
pub struct Fit {
    /// Envelope-domain coupling (NOT volume-normalized — the caller divides by its live vol_lin).
    pub g: f32,
    pub delay_samples: usize,
    pub skew_samples: i64,
    /// Per-leg matched-filter gains — the agreement evidence, logged for the field.
    pub g_up: f32,
    pub g_down: f32,
    /// Room floor (mean |sample| per 10ms bin) from the capture's quietest run.
    pub floor: f32,
    /// False = no peak stood above the correlation noise: a clean route (headset), g meaningless, floor still real.
    pub coupled: bool,
    /// The measured impulse response: taps[k] = IR at lag (ir_start + k), fitted against the SUM template — the NLMS canceller's seed (born converged; see call/nlms.rs).
    pub ir_start: usize,
    pub taps: Vec<f32>,
    /// Rough spectral coupling per band (SPECTRAL_BAND_EDGES_HZ), the same envelope-ratio unit as `g`.
    pub bands: [f32; 5],
}

/// Matched-filter one leg over the capture: (best lag, least-squares gain at the peak, peak-to-median-|corr| ratio). Polarity-blind (|dot| — speaker/mic chains can invert), integer MACs so the 2 × ~24k-lag × 12k-sample scan stays a fraction of a second off-thread.
fn leg_corr(cap: &[i16], leg: &[i16], max_lag: usize) -> (usize, f32, f64) {
    let n = leg.len();
    let scan = max_lag.min(cap.len().saturating_sub(n));
    let energy: f64 = leg.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>().max(1e-9);
    let mut mags: Vec<f64> = Vec::with_capacity(scan + 1);
    let mut best = (0usize, f64::MIN);
    for lag in 0..=scan {
        let dot: i64 = cap[lag..lag + n]
            .iter()
            .zip(leg)
            .map(|(&c, &r)| c as i32 as i64 * r as i64)
            .sum();
        let mag = (dot as f64).abs();
        mags.push(mag);
        if mag > best.1 {
            best = (lag, mag);
        }
    }
    mags.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = mags[mags.len() / 2].max(1e-9);
    (best.0, (best.1 / energy) as f32, best.1 / median)
}

/// Fit the capture against the template. `max_lag` is a parameter so tests scan short ranges; production passes [`MAX_LAG_SAMPLES`].
pub fn fit(cap: &[i16], max_lag: usize) -> Option<Fit> {
    if cap.len() < CHIRP_SAMPLES + BIN {
        return None;
    }
    // Envelope bins + floor first — needed by both verdicts.
    let cap_env: Vec<f32> = cap.chunks(BIN).filter(|c| c.len() == BIN).map(|c| env_i16(c)).collect();
    let floor = crate::call::calibrate::quietest_run(&cap_env, 20.min(cap_env.len().max(1)));
    // Reference legs at the EMITTED amplitude, i16-quantized (error ≪ any acoustic term).
    let leg_scale = built().1;
    let up = up_leg();
    let n = up.len();
    let ref_up: Vec<i16> = up.iter().map(|&v| (v * leg_scale) as i16).collect();
    let ref_down: Vec<i16> = (0..n).map(|i| ref_up[n - 1 - i]).collect();
    let (lag_up, g_up, psr_up) = leg_corr(cap, &ref_up, max_lag);
    let (lag_down, g_down, psr_down) = leg_corr(cap, &ref_down, max_lag);
    // No peak above the noise on either leg = clean route: legal (headset), floor is still a measurement.
    if psr_up < PSR_MIN || psr_down < PSR_MIN {
        return Some(Fit { g: 0.0, delay_samples: 0, skew_samples: 0, g_up, g_down, floor, coupled: false, ir_start: 0, taps: Vec::new(), bands: [0.0; 5] });
    }
    // Corruption gates: the two legs measured the same physics or the run is garbage. Reject details logged — two field calls said only "legs disagreed" and left nothing to diagnose which gate or by how much.
    let skew = lag_up as i64 - lag_down as i64;
    if skew.abs() > LEG_LAG_SPLIT_MAX {
        crate::logf!(
            "CALL: v-chirp reject — lag split {} samples (up {} down {}, gate {}); g {} / {}, psr {} / {}",
            skew,
            lag_up,
            lag_down,
            LEG_LAG_SPLIT_MAX,
            format!("{g_up:.4}"),
            format!("{g_down:.4}"),
            format!("{psr_up:.1}"),
            format!("{psr_down:.1}")
        );
        return None;
    }
    let (lo, hi) = (g_up.min(g_down).max(1e-6), g_up.max(g_down));
    if hi / lo > LEG_GAIN_RATIO_MAX {
        crate::logf!(
            "CALL: v-chirp reject — leg gain ratio {} (up {} down {}, gate {}); lags {} / {}, psr {} / {}",
            format!("{:.2}", hi / lo),
            format!("{g_up:.4}"),
            format!("{g_down:.4}"),
            LEG_GAIN_RATIO_MAX,
            lag_up,
            lag_down,
            format!("{psr_up:.1}"),
            format!("{psr_down:.1}")
        );
        return None;
    }
    let delay = ((lag_up + lag_down) / 2) as usize;
    // g in the learner's envelope unit: per-bin ratio of (mic − floor) to the template envelope at the matched delay, median over the chirp's active bins — median shrugs off a few "hello" bins where least-squares would inflate.
    let tpl_env: Vec<f32> = template().chunks(BIN).map(env_i16).collect();
    let off = delay / BIN;
    let mut ratios: Vec<f32> = Vec::with_capacity(tpl_env.len());
    let mut by_bin: Vec<(usize, f32)> = Vec::with_capacity(tpl_env.len());
    for (j, &te) in tpl_env.iter().enumerate() {
        if te < TPL_ACT {
            continue;
        }
        if let Some(&ce) = cap_env.get(j + off) {
            let r = ((ce - floor).max(0.0)) / te;
            ratios.push(r);
            by_bin.push((j, r));
        }
    }
    if ratios.is_empty() {
        return None;
    }
    ratios.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let g = ratios[ratios.len() / 2];
    let bands = spectral_bands(&by_bin);
    // IR export for the NLMS seed: cross-correlate the capture against the SUM template (what actually played) over the tap window around the matched delay. h[k] = <cap(lag), tpl>/|tpl|² — the least-squares IR at each lag, band-limited to the sweep (which is the whole audible path; fine, that's the band echo lives in).
    let tpl = template();
    let tpl_energy: f64 = tpl.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>().max(1e-9);
    let ir_start = delay.saturating_sub(crate::call::nlms::PRE);
    let mut taps: Vec<f32> = Vec::with_capacity(crate::call::nlms::TAPS);
    for k in 0..crate::call::nlms::TAPS {
        let lag = ir_start + k;
        let dot: i64 = if lag + tpl.len() <= cap.len() {
            cap[lag..lag + tpl.len()].iter().zip(tpl).map(|(&c, &t)| c as i64 * t as i64).sum()
        } else {
            0
        };
        taps.push((dot as f64 / tpl_energy) as f32);
    }
    Some(Fit { g, delay_samples: delay, skew_samples: skew, g_up, g_down, floor, coupled: true, ir_start, taps, bands })
}

/// The fit's answer for the LIVE engine, posted from the fit thread and drained by the engine loop (the persisted profile rides `calibrate::post_learned` separately).
pub enum Verdict {
    /// Real coupling: seed the predictive duck AND the NLMS canceller. g volume-normalized to unit volume (the duck re-scales by live vol_lin), delay on the duck's 10ms grid; `ir` = (ir_start, taps) in raw render↔mic units for the subtractor.
    Coupled { g_norm: f32, delay_bins: usize, floor: f32, ir: (usize, Vec<f32>) },
    /// No coupling above the correlation noise (headset-class route): stay reactive, but the floor is a real measurement.
    Clean { floor: f32 },
}

static VERDICT: std::sync::Mutex<Option<Verdict>> = std::sync::Mutex::new(None);

pub fn take_verdict() -> Option<Verdict> {
    VERDICT.lock().unwrap().take()
}

/// Run the fit off-thread (the 2-leg × ~24k-lag scan is ~100-500ms of CPU — never on the engine loop, which is pacing 10ms frames). `render_start_osc` = DAC-enqueue stamp of the chirp's first audible frame; `cap_anchor_osc` = osc of the capture buffer's first sample; their difference anchors the in-buffer lag to the true render→capture delay, so device spin-up before the chirp never inflates it.
pub fn finish(cap: Vec<i16>, vol_lin: f32, render_start_osc: i64, cap_anchor_osc: i64, route: String, emit_scale: f32) {
    let _ = std::thread::Builder::new().name("vchirp-fit".into()).spawn(move || {
        let t0 = std::time::Instant::now();
        let Some(f) = fit(&cap, MAX_LAG_SAMPLES) else {
            crate::log("CALL: v-chirp fit rejected — legs disagreed (speech/movement/resampler?) — no seed, duck stays on its prior");
            return;
        };
        if !f.coupled {
            crate::logf!(
                "CALL: v-chirp — clean route \"{}\" (no coupling above correlation noise; legs g {} / {}), floor {}, fit {}ms",
                route,
                format!("{:.4}", f.g_up),
                format!("{:.4}", f.g_down),
                format!("{:.0}", f.floor),
                t0.elapsed().as_millis()
            );
            *VERDICT.lock().unwrap() = Some(Verdict::Clean { floor: f.floor });
            return;
        }
        let ops = vsf::OSCILLATIONS_PER_SECOND as f64;
        let lag_osc = cap_anchor_osc as f64 + f.delay_samples as f64 * ops / SAMPLE_RATE as f64
            - render_start_osc as f64;
        let anchor_ms = (cap_anchor_osc - render_start_osc) as f64 / ops * 1000.0;
        let acoustic_ms = f.delay_samples as f64 * 1000.0 / SAMPLE_RATE as f64;
        let delay_bins_raw = (lag_osc / ops * 100.0).round().max(0.0) as usize;
        // A delay AT the grid's bound is not a room, it is the two clock anchors disagreeing (Brittany 2026-09-09: 1500 ms with a 1.4 s capture-anchor offset — the duck then muted her 1.5 s after every phrase of Nick's). No seed, nothing persisted; the reactive duck and the learner carry the call.
        if delay_bins_raw >= 150 {
            crate::logf!(
                "CALL: v-chirp reject — delay {}ms at the grid bound (capture anchor {}ms after render start, acoustic lag {}ms, legs g {} / {}): clock anchors disagree, no seed",
                delay_bins_raw * 10,
                format!("{anchor_ms:.0}"),
                format!("{acoustic_ms:.1}"),
                format!("{:.4}", f.g_up),
                format!("{:.4}", f.g_down)
            );
            return;
        }
        let delay_bins = delay_bins_raw;
        // Divide out the digital probe boost (the fit correlates against the unscaled template) and the stream volume — g publishes at unit volume, unit scale.
        let scale = emit_scale.max(1e-6);
        let g_norm = if vol_lin > 0.0 { f.g / scale / vol_lin } else { f.g / scale };
        let taps: Vec<f32> = f.taps.iter().map(|&t| t / scale).collect();
        // THE SEED'S LAG IS THE PHYSICAL RENDER→CAPTURE DELAY, on the reference timeline (nlms.rs: `from = frame_pos − ir_start − n + 1`), NOT the chirp's position inside the capture buffer — the capture starts at probe start, the sweep sits a quarter-second pad later (2026-09-10 Azie/Nick: the position-as-lag seed sat 250 ms wrong; Nick's filter never found its reference window, Azie's adapted from a wrong seed and read −17 dB). lag_osc already holds the physical delay; convert it to samples and back off by the pre-roll.
        let lag_ref_samples = (lag_osc / ops * SAMPLE_RATE as f64).round().max(0.0) as usize;
        let ir_start = lag_ref_samples.saturating_sub(crate::call::nlms::PRE);
        crate::logf!(
            "CALL: v-chirp — g {} delay {}ms (anchor {}ms + capture pos {}ms) skew {} sample(s) (legs g {} / {}), floor {}, route \"{}\", fit {}ms; spectral 200-500 {} / 500-1k2 {} / 1k2-3k {} / 3k-8k {} / 8k-20k {}",
            format!("{g_norm:.4}"),
            delay_bins * 10,
            format!("{anchor_ms:.0}"),
            format!("{acoustic_ms:.1}"),
            f.skew_samples,
            format!("{:.4}", f.g_up),
            format!("{:.4}", f.g_down),
            format!("{:.0}", f.floor),
            route,
            t0.elapsed().as_millis(),
            format!("{:.3}", f.bands[0] / scale),
            format!("{:.3}", f.bands[1] / scale),
            format!("{:.3}", f.bands[2] / scale),
            format!("{:.3}", f.bands[3] / scale),
            format!("{:.3}", f.bands[4] / scale)
        );
        // Persist thru the learned-profile drain (same blend as the in-call learner, solid tier — a fresh direct measurement of THIS route).
        crate::call::calibrate::post_learned(vec![crate::call::calibrate::LearnedResult {
            result: crate::call::calibrate::CalResult::Echo(crate::call::calibrate::EchoProfile {
                g_norm,
                delay_ms: (delay_bins * 10) as u32,
                cal_vol_db: crate::platform::audio::current_volume_db().map(|_| 0.0),
                route_id: route,
            }),
            windows: 25,
            solid: true,
        }]);
        *VERDICT.lock().unwrap() = Some(Verdict::Coupled { g_norm, delay_bins, floor: f.floor, ir: (ir_start, taps) });
    });
}

fn env_i16(frame: &[i16]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    frame.iter().map(|s| s.unsigned_abs() as u64).sum::<u64>() as f32 / frame.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    // Deterministic noise for KATs (Date-free, seed-fixed — same law as the engine's no-wallclock rule).
    fn noise(len: usize, amp: i16, seed: u64) -> Vec<i16> {
        let mut s = seed;
        (0..len)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((s >> 33) as i64 % (2 * amp as i64 + 1) - amp as i64) as i16
            })
            .collect()
    }

    #[test]
    fn template_shape() {
        let t = template();
        assert_eq!(t.len(), CHIRP_SAMPLES);
        // Both ends on zero crossings — the spec's "starts from 0", both legs, both ends.
        assert!(t[0].abs() < 200 && t[t.len() - 1].abs() < 200, "ends {} {}", t[0], t[t.len() - 1]);
        let peak = t.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!(peak as f64 <= PEAK_TARGET + 1.0 && peak as f64 > PEAK_TARGET * 0.9, "peak {peak}");
    }

    /// Build a synthetic room capture: silence, then the template at `gain` and `delay`, plus floor noise.
    fn synth(delay: usize, gain: f32, seed: u64) -> Vec<i16> {
        let mut cap = noise(delay + CHIRP_SAMPLES + 9600, 40, seed);
        for (i, &s) in template().iter().enumerate() {
            cap[delay + i] = (cap[delay + i] as f32 + s as f32 * gain).clamp(-32768.0, 32767.0) as i16;
        }
        cap
    }

    #[test]
    fn probe_is_one_second_with_the_sweep_in_the_middle() {
        let fr = frames();
        let total: usize = fr.iter().map(|f| f.len()).sum();
        assert_eq!(total, SAMPLE_RATE, "one second emitted");
        let flat: Vec<i16> = fr.concat();
        assert!(flat[..PAD_SAMPLES].iter().all(|&v| v == 0), "leading pad silent");
        assert!(flat[PAD_SAMPLES + CHIRP_SAMPLES..].iter().all(|&v| v == 0), "trailing pad silent");
        assert_eq!(&flat[PAD_SAMPLES..PAD_SAMPLES + CHIRP_SAMPLES], template());
        assert_eq!(CHIRP_SAMPLES * 2, SAMPLE_RATE);
    }

    #[test]
    fn spectral_bands_take_the_median_per_band() {
        let n = CHIRP_SAMPLES / BIN;
        let by_bin: Vec<(usize, f32)> = (0..n).map(|j| (j, j as f32)).collect();
        let b = spectral_bands(&by_bin);
        assert!(b[0] < b[1] && b[1] < b[2] && b[2] < b[3] && b[3] < b[4], "rising sweep index per band: {b:?}");
        assert_eq!(spectral_bands(&[]), [0.0; 5]);
    }

    #[test]
    fn fit_recovers_delay_and_gain() {
        let cap = synth(3000, 0.3, 7);
        let f = fit(&cap, 6000).expect("fit");
        assert!(f.coupled, "coupled");
        assert!((f.delay_samples as i64 - 3000).abs() <= 2, "delay {}", f.delay_samples);
        assert!(f.skew_samples.abs() <= 2, "skew {}", f.skew_samples);
        assert!((f.g - 0.3).abs() < 0.09, "g {}", f.g);
        assert!((f.g_up - 0.3).abs() < 0.09 && (f.g_down - 0.3).abs() < 0.09, "legs {} {}", f.g_up, f.g_down);
    }

    #[test]
    fn fit_survives_hello_overlay() {
        // A loud low "hello" hum right over the echo — the answer-moment hazard the probe must shrug off.
        let mut cap = synth(2400, 0.2, 11);
        for i in 0..4800usize {
            let v = (8000.0 * (std::f64::consts::TAU * 220.0 * i as f64 / SAMPLE_RATE as f64).sin()) as i16;
            let j = 2400 + i;
            cap[j] = (cap[j] as i32 + v as i32).clamp(-32768, 32767) as i16;
        }
        let f = fit(&cap, 6000).expect("fit");
        assert!(f.coupled);
        assert!((f.delay_samples as i64 - 2400).abs() <= 4, "delay {}", f.delay_samples);
        // The median-ratio g may pick up some speech bins; it must stay same-order, never collapse or explode.
        assert!(f.g > 0.1 && f.g < 0.5, "g {}", f.g);
    }

    #[test]
    fn clean_room_reads_uncoupled() {
        let cap = noise(CHIRP_SAMPLES + 9600, 40, 13);
        let f = fit(&cap, 6000).expect("fit");
        assert!(!f.coupled, "noise must not fake a coupling (g_up {} g_down {})", f.g_up, f.g_down);
        assert!(f.floor > 0.0 && f.floor < 60.0, "floor {}", f.floor);
    }

    #[test]
    fn headset_trace_coupling_still_measures() {
        // "every device leaks, it's just how much" — a 1% coupling must still resolve, not read as clean.
        let cap = synth(1200, 0.01, 17);
        let f = fit(&cap, 3000).expect("fit");
        assert!(f.coupled, "1% coupling should stand above noise (psr gate)");
        assert!((f.delay_samples as i64 - 1200).abs() <= 4, "delay {}", f.delay_samples);
        assert!(f.g < 0.05, "g {}", f.g);
    }
}
