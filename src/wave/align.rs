//! Sender-side alignment to true time (docs/lock.md §5, docs/waves.md "One spool per identity"): the captured stream is stretched or shrunk one sample at a time so that the sample the microphone heard nearest grid slot `k` is emitted AS sample `k`, then cut into frames named on the absolute 5 ms grid.
//! Crystal drift is corrected here, at the source, and nowhere else on the capture side — every party's channel then lines up at the instant its microphone heard it.
//! Pure: the caller hands in samples, their hardware frame position, and the HAL's (frame, true time) pairs already mapped thru TrueClock; nothing here reads a clock.
//! Integers name every sample and frame; floats live only inside the rate regression and the phase loop (the spec's estimator exemption).

use std::collections::VecDeque;

/// Samples per second on the grid (spec §1.2).
pub const RATE: i64 = 48_000;
/// The engine's frame, and so the grid step every frame name sits on: 5 ms (a quarter of the spec's 20 ms packet).
pub const FRAME: i64 = 240;
const OPS: f64 = crate::OSC_PER_SEC as f64;

/// The name of the frame containing grid sample `k`.
pub fn frame_start(k: i64) -> i64 {
    k.div_euclid(FRAME) * FRAME
}

/// The capture-rate regression (spec §5.2): true time against hardware frame position over a sliding 10 s window, so the HAL's timestamp jitter never reaches the phase loop.
#[derive(Clone, Debug, Default)]
pub struct RateFit {
    pts: VecDeque<(i64, i64)>,
    /// eagle = a + b·(frame − f_ref), refitted on every new pair.
    f_ref: i64,
    a: f64,
    b: f64,
    resid_osc: f64,
}

/// The regression window, in hardware frames.
const FIT_WINDOW_FRAMES: i64 = 10 * RATE;

impl RateFit {
    /// Add one HAL pair (hardware frame position, its true capture time in Eagle oscillations) and refit. A repeated frame position (the HAL updates its timestamp per burst, not per callback) is ignored.
    pub fn push(&mut self, frame: i64, eagle: i64) {
        if self.pts.back().is_some_and(|&(f, _)| f >= frame) {
            return;
        }
        self.pts.push_back((frame, eagle));
        while self.pts.front().is_some_and(|&(f, _)| frame - f > FIT_WINDOW_FRAMES) {
            self.pts.pop_front();
        }
        self.refit();
    }

    fn refit(&mut self) {
        let n = self.pts.len() as f64;
        let (f0, e0) = self.pts[0];
        let mf = self.pts.iter().map(|&(f, _)| (f - f0) as f64).sum::<f64>() / n;
        let me = self.pts.iter().map(|&(_, e)| (e - e0) as f64).sum::<f64>() / n;
        let (mut sxx, mut sxy) = (0.0, 0.0);
        for &(f, e) in &self.pts {
            let (x, y) = ((f - f0) as f64 - mf, (e - e0) as f64 - me);
            sxx += x * x;
            sxy += x * y;
        }
        // One pair (or pairs at one position) has no slope yet: assume the nominal rate until a second position arrives.
        self.b = if sxx > 0.0 { sxy / sxx } else { OPS / RATE as f64 };
        self.f_ref = f0 + mf.round() as i64;
        self.a = e0 as f64 + me + self.b * (self.f_ref - f0) as f64 - self.b * mf;
        let var = self
            .pts
            .iter()
            .map(|&(f, e)| {
                let pred = self.a + self.b * (f - self.f_ref) as f64;
                (e as f64 - pred).powi(2)
            })
            .sum::<f64>()
            / n;
        self.resid_osc = var.sqrt();
    }

    /// The true capture time of hardware frame `frame`, from the fit; `None` before the first pair.
    pub fn eagle_of(&self, frame: i64) -> Option<i64> {
        if self.pts.is_empty() {
            return None;
        }
        Some((self.a + self.b * (frame - self.f_ref) as f64).round() as i64)
    }

    /// The ADC's rate error against true time, parts per million (spec §5.2 `adc_ppm`): positive = the crystal runs fast. `None` until two positions are in.
    pub fn adc_ppm(&self) -> Option<f64> {
        (self.pts.len() >= 2).then(|| ((OPS / RATE as f64) / self.b - 1.0) * 1e6)
    }

    /// Timestamp quality: the regression's residual, nanoseconds (spec §5.2).
    pub fn residual_ns(&self) -> u64 {
        (self.resid_osc * 1e9 / OPS) as u64
    }
}

/// One corrector command (spec §5.4, the slip implementation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slip {
    Insert,
    Delete,
}

/// Samples at each buffer edge a slip never touches (spec §5.4).
const SLIP_EDGE: usize = 8;

/// Apply one slip to `buf` in place at the flattest, quietest point: minimise |x[n] − x[n−1]| + ¼|x[n]| over the interior (spec §5.4), duplicating or removing that sample. `false` when the buffer is too short to hold a slip away from its edges — the caller carries the command to the next buffer.
pub fn apply_slip(buf: &mut Vec<i32>, slip: Slip) -> bool {
    if buf.len() < 2 * SLIP_EDGE + 1 {
        return false;
    }
    let cost = |n: usize| (buf[n] as i64 - buf[n - 1] as i64).abs() * 4 + (buf[n] as i64).abs();
    let n = (SLIP_EDGE..buf.len() - SLIP_EDGE).min_by_key(|&n| cost(n)).expect("interior is non-empty (length checked above)");
    match slip {
        Slip::Insert => buf.insert(n, buf[n]),
        Slip::Delete => {
            buf.remove(n);
        }
    }
    true
}

/// Loop constants (spec §5.4): proportional and integral gains, the deadband in samples, the error low-pass, and the loop period in samples (100 ms).
const KP: f64 = 0.05;
const KI: f64 = 0.002;
const DEADBAND: f64 = 0.5;
const TAU_S: f64 = 1.0;
const LOOP_SAMPLES: i64 = RATE / 10;

/// What one push reports for the per-second telemetry (spec §8): the measured phase, the slips so far, the fit's view of the crystal.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AlignStats {
    /// The raw phase of the newest buffer: a one-sample sawtooth between slips, by construction.
    pub phase_samples: f64,
    /// The low-passed phase the loop holds (τ 1 s) — the spec's §9.3 quantity.
    pub phase_filtered: f64,
    pub slips_inserted: u64,
    pub slips_deleted: u64,
    pub adc_ppm: Option<f64>,
    pub residual_ns: u64,
    /// Buffers dropped before the first HAL timestamp arrived (the spec forbids naming samples by callback time).
    pub unnamed_dropped: u64,
}

/// The sender's aligner: HAL pairs in, grid-named frames out.
#[derive(Clone, Debug, Default)]
pub struct Aligner {
    fit: RateFit,
    /// Grid name of `pending[0]`; `None` until the first named sample.
    next_k: Option<i64>,
    /// Samples not yet cut into a frame, in the capture's 24-bit domain (i32).
    pending: Vec<i32>,
    filt: f64,
    integ: f64,
    /// Correction ordered but not yet realized as whole slips (samples): the loop's output is a RATE, and slips deliver it one sample at a time (sigma-delta).
    acc: f64,
    since_loop: i64,
    /// A slip the loop ordered that no buffer has carried yet.
    owed: Option<Slip>,
    pub stats: AlignStats,
}

impl Aligner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one HAL timestamp pair (hardware frame position, true capture time).
    pub fn timestamp(&mut self, frame: i64, eagle: i64) {
        self.fit.push(frame, eagle);
        self.stats.adc_ppm = self.fit.adc_ppm();
        self.stats.residual_ns = self.fit.residual_ns();
    }

    /// One captured buffer whose first sample is hardware frame `first_frame`. Completed frames are appended to `out` as `(k0, samples)`, `k0 % FRAME == 0`, contiguous from the first.
    pub fn push(&mut self, input: &[i32], first_frame: i64, out: &mut Vec<(i64, Vec<i32>)>) {
        let Some(measured) = self.fit.eagle_of(first_frame) else {
            self.stats.unnamed_dropped += 1;
            return;
        };
        let next_k = match self.next_k {
            Some(k) => k,
            None => {
                // THE FIRST FRAME IS PADDED (Nick 2026-09-25): the first captured sample takes the grid name nearest its true instant, and the part of its frame before it is zeros — the wave starts the instant the human speaks, on a grid boundary.
                let k = vsf::grid::eagle_to_sample(measured);
                let f0 = frame_start(k);
                self.pending = vec![0; (k - f0) as usize]; // PROOF: frame_start(k) ≤ k < frame_start(k) + FRAME
                self.next_k = Some(f0);
                f0
            }
        };
        // The name this buffer's first sample is about to receive, against the instant it was actually captured.
        let named = next_k + self.pending.len() as i64;
        let phase = vsf::grid::phase_error_samples(named, measured);
        self.stats.phase_samples = phase;
        let dt = input.len() as f64 / RATE as f64;
        self.filt += (phase - self.filt) * (dt / TAU_S).min(1.0);
        self.stats.phase_filtered = self.filt;
        self.since_loop += input.len() as i64;
        if self.since_loop >= LOOP_SAMPLES {
            self.since_loop = 0;
            // PI on the filtered phase; the output is samples of correction per 100 ms tick (discrete loop: ζ ≈ kp / 2√ki ≈ 0.56, settling ≈ 16 s — inside the spec's 30 s).
            self.integ += self.filt * KI;
            self.acc += self.filt * KP + self.integ;
            // Captured LATER than its name says (positive phase) ⇒ the names run early ⇒ insert a sample to push the rest later; earlier ⇒ delete one. At most one slip per tick; the deadband keeps a half-sample of accumulated correction from dithering.
            if self.owed.is_none() {
                if self.acc > DEADBAND {
                    self.owed = Some(Slip::Insert);
                    self.acc -= 1.0;
                } else if self.acc < -DEADBAND {
                    self.owed = Some(Slip::Delete);
                    self.acc += 1.0;
                }
            }
        }
        let mut buf = input.to_vec();
        if let Some(s) = self.owed {
            if apply_slip(&mut buf, s) {
                self.owed = None;
                match s {
                    Slip::Insert => self.stats.slips_inserted += 1,
                    Slip::Delete => self.stats.slips_deleted += 1,
                }
            }
        }
        self.pending.extend_from_slice(&buf);
        let mut k = next_k;
        let whole = self.pending.len() / FRAME as usize * FRAME as usize;
        for chunk in self.pending[..whole].chunks_exact(FRAME as usize) {
            out.push((k, chunk.to_vec()));
            k += FRAME;
        }
        self.pending.drain(..whole);
        self.next_k = Some(k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: i64 = crate::OSC_PER_SEC;

    /// A simulated ADC running `ppm` fast from true instant `e0`: hardware frame f was captured at e0 + f·T/(1+ppm).
    struct Adc {
        e0: i64,
        ppm: f64,
        frame: i64,
    }

    impl Adc {
        fn eagle_of(&self, f: i64) -> i64 {
            self.e0 + (f as f64 * (S as f64 / RATE as f64) / (1.0 + self.ppm * 1e-6)).round() as i64
        }
        /// Run `secs` seconds of 2 ms bursts of `sig` thru the aligner, a HAL pair every burst.
        fn run(&mut self, a: &mut Aligner, secs: i64, sig: &dyn Fn(i64) -> i32, out: &mut Vec<(i64, Vec<i32>)>, mut each: impl FnMut(&Aligner)) {
            let burst = 96;
            for _ in 0..(secs * RATE / burst) {
                a.timestamp(self.frame, self.eagle_of(self.frame));
                let buf: Vec<i32> = (self.frame..self.frame + burst).map(sig).collect();
                a.push(&buf, self.frame, out);
                self.frame += burst;
                each(a);
            }
        }
    }

    /// The first frame is padded to its grid boundary, every frame is named on the grid, and names run contiguous.
    #[test]
    fn frames_are_named_on_the_grid_and_the_first_is_padded() {
        // A true start 1.7 ms past a frame boundary.
        let e0 = vsf::grid::sample_to_eagle(1_000_000 * FRAME + 82);
        let mut adc = Adc { e0, ppm: 0.0, frame: 0 };
        let (mut a, mut out) = (Aligner::new(), Vec::new());
        adc.run(&mut a, 1, &|_| 1000, &mut out, |_| {});
        assert_eq!(out[0].0, 1_000_000 * FRAME);
        assert!(out[0].1[..82].iter().all(|&x| x == 0), "the part before the first captured sample is zeros");
        assert_eq!(out[0].1[82], 1000, "the first captured sample lands on its own slot");
        assert!(out.windows(2).all(|w| w[1].0 == w[0].0 + FRAME && w[0].0 % FRAME == 0));
    }

    /// Spec §9.3: an ADC at +80 ppm is held within half a sample of true time inside 30 s and stays there, at ≈ 3.84 deletions a second.
    #[test]
    fn a_fast_crystal_is_held_on_true_time() {
        let mut adc = Adc { e0: 1_790_000_000 * S, ppm: 80.0, frame: 0 };
        let (mut a, mut out) = (Aligner::new(), Vec::new());
        let (mut worst_after_30, mut raw_peak) = (0.0f64, 0.0f64);
        let mut t = 0i64;
        adc.run(&mut a, 120, &|f| ((f * 37) % 2000 - 1000) as i32, &mut out, |a| {
            t += 96;
            if t > 30 * RATE {
                worst_after_30 = worst_after_30.max(a.stats.phase_filtered.abs());
                raw_peak = raw_peak.max(a.stats.phase_samples.abs());
            }
        });
        eprintln!("after 30 s: held phase peak {worst_after_30:.3} samples, raw sawtooth peak {raw_peak:.3}");
        assert!(worst_after_30 < 0.5, "the held phase peaked at {worst_after_30} samples");
        assert!(raw_peak < 1.0, "a slip corrector's raw phase walks one sample between slips, never more: {raw_peak}");
        let ppm = a.stats.adc_ppm.unwrap();
        assert!((ppm - 80.0).abs() < 0.5, "the fit sees the crystal: {ppm} ppm");
        let rate = a.stats.slips_deleted as f64 / 120.0;
        assert!((rate - 3.84).abs() < 0.3, "{rate} deletions a second");
        assert_eq!(a.stats.slips_inserted, 0);
    }

    /// Timestamp jitter never reaches the loop (spec §5.4): ±250 µs of noise on every HAL pair, and the held phase stays under half a sample — the 10 s regression absorbs it.
    #[test]
    fn hal_jitter_is_absorbed_by_the_regression() {
        let (mut a, mut out) = (Aligner::new(), Vec::new());
        let (e0, ppm) = (1_790_000_000 * S, 30.0);
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let (mut f, mut worst) = (0i64, 0.0f64);
        for i in 0..(60 * RATE / 96) {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); // the algorithm: an LCG, wrapping by definition
            let jitter = ((seed >> 33) as i64 % 501 - 250) * S / 1_000_000;
            let truth = e0 + (f as f64 * (S as f64 / RATE as f64) / (1.0 + ppm * 1e-6)).round() as i64;
            a.timestamp(f, truth + jitter);
            a.push(&[0i32; 96], f, &mut out);
            f += 96;
            if i as i64 * 96 > 30 * RATE {
                worst = worst.max(a.stats.phase_filtered.abs());
            }
        }
        assert!(worst < 0.5, "held phase {worst} samples under jitter");
        assert!(a.stats.residual_ns > 100_000, "the residual reports the jitter it absorbed: {} ns", a.stats.residual_ns);
    }

    /// And a slow crystal is held by insertions.
    #[test]
    fn a_slow_crystal_is_held_by_insertions() {
        let mut adc = Adc { e0: 1_790_000_000 * S, ppm: -50.0, frame: 0 };
        let (mut a, mut out) = (Aligner::new(), Vec::new());
        adc.run(&mut a, 60, &|_| 0, &mut out, |_| {});
        assert!(a.stats.phase_samples.abs() < 1.0);
        assert!(a.stats.slips_inserted > 100 && a.stats.slips_deleted < a.stats.slips_inserted / 10);
    }

    /// THD+N of a slipped sine (spec §9.3): one best-fit sinusoid per 50 ms block, residual against signal, for 4 slips a second.
    fn thd_n_db(freq: f64) -> f64 {
        let amp = 16_000.0;
        let x: Vec<i32> = (0..RATE * 2).map(|i| (amp * (2.0 * std::f64::consts::PI * freq * i as f64 / RATE as f64).sin()) as i32).collect();
        let mut y: Vec<i32> = Vec::new();
        for (i, chunk) in x.chunks(96).enumerate() {
            let mut b = chunk.to_vec();
            if i % 125 == 60 {
                apply_slip(&mut b, if (i / 125) % 2 == 0 { Slip::Insert } else { Slip::Delete });
            }
            y.extend_from_slice(&b);
        }
        let (mut sig, mut res) = (0.0f64, 0.0f64);
        for blk in y.chunks_exact(2400) {
            let w = 2.0 * std::f64::consts::PI * freq / RATE as f64;
            let (mut ss, mut sc, mut cc, mut ys, mut yc) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for (n, &v) in blk.iter().enumerate() {
                let (s, c) = ((w * n as f64).sin(), (w * n as f64).cos());
                ss += s * s;
                sc += s * c;
                cc += c * c;
                ys += v as f64 * s;
                yc += v as f64 * c;
            }
            let det = ss * cc - sc * sc;
            let (p, q) = ((ys * cc - yc * sc) / det, (yc * ss - ys * sc) / det);
            for (n, &v) in blk.iter().enumerate() {
                let fit = p * (w * n as f64).sin() + q * (w * n as f64).cos();
                sig += fit * fit;
                res += (v as f64 - fit).powi(2);
            }
        }
        10.0 * (res / sig).log10()
    }

    /// 200 Hz must stay under −40 dB (spec §9.3); 2 kHz is documented, expected worse — the trigger for the fractional resampler.
    #[test]
    fn slips_are_quiet_on_voice_band_tones() {
        let low = thd_n_db(200.0);
        assert!(low < -40.0, "200 Hz THD+N {low:.1} dB");
        let high = thd_n_db(2000.0);
        eprintln!("slip THD+N: 200 Hz {low:.1} dB, 2 kHz {high:.1} dB");
    }
}
