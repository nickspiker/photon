//! Chirp-seeded NLMS echo canceller (Nick 2026-09-08: "go for the NLMS... since we have a profile from the connect chirp, no need for RLS"). Pure math — the engine owns the wiring.
//!
//! The design in one paragraph: the v-chirp probe measures the speaker→mic impulse response at every connect, so the filter is BORN converged — NLMS's one weakness (slow convergence on colored signals) never applies, and RLS's O(L²) selling point buys nothing. The canceller is causal and inline: `residual[n] = mic[n] − Σ h[k]·ref[n − ir_start − k]`, zero added samples of latency — the cost is CPU only (~2×TAPS MACs per sample, a few percent of one core at 48k). Adaptation is gated by the caller on the far-talks-alone classification the duck already computes; frozen under double-talk (the standard divergence defense). The duck demotes to RESIDUAL suppressor while a filter is armed — the moment calls become genuinely full-duplex.
//!
//! Alignment: mic and reference each advance by pure SAMPLE COUNT from their session anchors (contiguous streams), so per-frame stamp wobble never smears the filter; the one-time anchor offset and any true clock skew land inside the filter's PRE-roll window and NLMS tracks the slow residual drift. A route swap disarms (new physics — the next call's chirp re-seeds).

/// Filter length: ~42.7ms of echo path at 48k — room tail on a speakerphone, anchored at the chirp-measured delay.
pub const TAPS: usize = 2048;
/// Taps AHEAD of the measured anchor: absorbs anchor wobble (±ms of drain-stamp jitter folded into the one-time alignment) and slow clock skew.
pub const PRE: usize = 512;
/// NLMS step size — conservative; the seed does the converging, this tracks drift.
const MU: f32 = 0.5;
/// PRE-WHITENING (Nick 2026-09-10: "the half, quarter, eighth… filter from 512Hz up and flatten out the profile"). NLMS converges at the rate of the WEAKEST band, and speech falls ~6dB/octave above ~500Hz, so the taps learn the bass echo and never the top — the 8dB ERLE ceiling of the field. The whitener is a sum of dyadic differences x[n] − x[n−s] at these sample scales, equal weight: a comb of high-passes that flattens speech from ~500Hz up while leaving the bass its share. Cheap because the whitener is LTI and commutes with the echo path: the whitened error EQUALS the whitening of the raw error, so the one convolution stays on raw audio (that residual is what plays out) and only the tap UPDATE sees whitened signals — a whitened reference window (computed per frame from the same ring, a few adds per sample) and a whitened error (a 16-sample history). Filtered-error NLMS, no second convolution, no latency.
pub const WHITEN_SCALES: [usize; 3] = [1, 4, 16];
/// Longest whitener lag — the history the reference window and the error keep.
const WHITEN_SPAN: usize = 16;

/// The whitener over a contiguous signal: y[i] = mean over scales of (x[i] − x[i−s]); `x` carries WHITEN_SPAN samples of pre-history, so the output has `x.len() − WHITEN_SPAN` samples.
fn whiten(x: &[f32]) -> Vec<f32> {
    let k = WHITEN_SCALES.len() as f32;
    (WHITEN_SPAN..x.len())
        .map(|i| WHITEN_SCALES.iter().map(|&s| x[i] - x[i - s]).sum::<f32>() / k)
        .collect()
} // 2026-09-09: was 0.25 — the field's echo was audible with the filter barely moving; NLMS is stable below 2, and the per-sample update is what tracks a drifting path
/// Regularisation floor on the (whitened) reference window's power, per tap: NLMS divides the step by that power, and a window of digital silence (the far end's mute-transmits-zeros, a DAC pulling silence) has NONE — with the old 1e3 the step became MU × error ÷ 1e3 and one loud near-end frame thru an open gate multiplied the taps into the millions (field 2026-09-10, the first whitener build: −143dB "ERLE" on one phone, NaN on the other, the near mic silenced for the rest of the wave). A floor of the noise-amplitude-squared per tap keeps the step bounded whatever the gate lets thru; a window under it is not adapted on at all.
const POWER_FLOOR_PER_TAP: f32 = 400.0; // amplitude ~20 of i16, squared

/// Flat reference ring over what the DAC actually rendered, indexed by ABSOLUTE sample position (never wraps indices — the buffer slides).
pub struct RefRing {
    buf: Vec<f32>,
    /// Absolute position of buf[0].
    start: u64,
    /// Absolute position one past the last sample.
    end: u64,
    cap: usize,
}

impl RefRing {
    pub fn new(cap: usize) -> Self {
        Self { buf: Vec::with_capacity(cap), start: 0, end: 0, cap }
    }

    pub fn push(&mut self, frame: &[i16]) {
        self.buf.extend(frame.iter().map(|&s| s as f32));
        self.end += frame.len() as u64;
        if self.buf.len() > self.cap {
            let drop = self.buf.len() - self.cap;
            self.buf.drain(..drop);
            self.start += drop as u64;
        }
    }

    pub fn end(&self) -> u64 {
        self.end
    }

    /// A contiguous window [from, from+len) as a slice, or None if any of it has aged out / not arrived (the caller skips the frame — never zero-padded guesswork).
    fn window(&self, from: i64, len: usize) -> Option<&[f32]> {
        if from < self.start as i64 || (from + len as i64) > self.end as i64 {
            return None;
        }
        let i = (from - self.start as i64) as usize;
        Some(&self.buf[i..i + len])
    }
}

pub struct Nlms {
    /// h[k] = IR at lag (ir_start + k) — seeded verbatim from the chirp fit's exported taps.
    h: Vec<f32>,
    ir_start: i64,
    /// Telemetry: Σ|mic|² and Σ|residual|² over ADAPT-gated (far-talks-alone) frames — the honest ERLE, measured only where echo dominates.
    pub pre_e: f64,
    pub post_e: f64,
    pub adapted_frames: u64,
    /// RECENT ERLE (exponential, ~100 adapted frames): the self-check judges the filter on what it is doing NOW, not its lifetime — a blunt seed that starts harmful and converges must not be shot for its first second.
    recent_pre: f64,
    recent_post: f64,
    /// The last WHITEN_SPAN raw errors (oldest first) — the whitened error's history across frame edges.
    err_hist: [f32; WHITEN_SPAN],
    /// Frames the filter ran on at all (adapted or frozen) — with `adapted_frames`, the adapt ratio the stats print.
    pub run_frames: u64,
    /// The taps went non-finite: the filter passes raw audio thru from then on and reports itself harmful so the engine disarms it. Logged once by the engine.
    pub diverged: bool,
}

impl Nlms {
    pub fn new(ir_start: usize, taps: Vec<f32>) -> Self {
        Self { h: taps, ir_start: ir_start as i64, pre_e: 0.0, post_e: 0.0, adapted_frames: 0, recent_pre: 0.0, recent_post: 0.0, err_hist: [0.0; WHITEN_SPAN], run_frames: 0, diverged: false }
    }

    /// ERLE in dB over the adapted stretches; None until any frame adapted.
    pub fn erle_db(&self) -> Option<f64> {
        (self.adapted_frames > 0 && self.post_e > 0.0)
            .then(|| 10.0 * (self.pre_e / self.post_e).log10())
    }

    /// A canceller that MADE ECHO WORSE must be shot (field 2026-09-08: a −43dB chirp barely passed the fit gate and seeded a misaligned filter → −17.8dB ERLE, i.e. +17.8dB of injected garbage = the "scratchy"). After a probation window of adapted frames, net-negative ERLE means the seed was garbage — the caller disarms and falls back to the duck. `false` until probation completes (never judge on one noisy frame).
    pub fn is_net_harmful(&self) -> bool {
        const PROBATION: u64 = 200; // ~2s of far-talk-alone adaptation before any verdict
        self.diverged || (self.adapted_frames >= PROBATION && self.recent_post > self.recent_pre)
    }

    /// ERLE over the recent adapted stretch (the self-check's view); None until adapted.
    pub fn erle_recent_db(&self) -> Option<f64> {
        (self.adapted_frames > 0 && self.recent_post > 0.0).then(|| 10.0 * (self.recent_pre / self.recent_post).log10())
    }

    /// Cancel one mic frame in place. `frame_pos` = the frame's first sample in the REFERENCE timeline (mic count + one-time anchor offset). `adapt` = the far-talks-alone gate. `ref_gain` = vol_lin_now ÷ vol_lin_at_seed — the taps are measured at the probe's volume, and the DAC gain sits between the reference and the room, so a mid-call volume change scales the echo without touching h; folding the ratio into the reference keeps the filter honest instantly (adaptation then refines in seed-volume units). Frames whose reference window isn't fully resident pass thru untouched.
    pub fn cancel_frame(&mut self, mic: &mut [i16], ring: &RefRing, frame_pos: i64, adapt: bool, ref_gain: f32) {
        let n = self.h.len();
        let len = mic.len();
        // The whole frame's reference span plus the whitener's pre-history: oldest sample needed is (frame_pos − ir_start − n + 1 − WHITEN_SPAN), newest is (frame_pos + len − 1 − ir_start).
        let from = frame_pos - self.ir_start - n as i64 + 1;
        let Some(win_ext) = ring.window(from - WHITEN_SPAN as i64, n + len - 1 + WHITEN_SPAN) else {
            return;
        };
        let win = &win_ext[WHITEN_SPAN..];
        self.run_frames += 1;
        if self.diverged {
            return; // raw passes thru; the engine disarms on the next self-check
        }
        // Whitened reference (aligned with `win`) and the whitened window's power as a running sum — both only when adapting. A window whose whitened power sits under the floor is not adapted on: there is no reference to learn from there, only near-end sound to poison the taps with.
        let floor = POWER_FLOOR_PER_TAP * n as f32;
        let g2 = ref_gain * ref_gain;
        let xw: Vec<f32> = if adapt { whiten(win_ext) } else { Vec::new() };
        let mut power: f32 = if adapt { xw[..n].iter().map(|&r| r * r).sum() } else { 0.0 };
        let adapting = adapt && power * g2 >= floor;
        // Error history for the whitened error: WHITEN_SPAN past raw errors, then this frame's, appended sample by sample.
        let mut e_hist: Vec<f32> = Vec::with_capacity(WHITEN_SPAN + len);
        e_hist.extend_from_slice(&self.err_hist);
        let mut pre = 0f64;
        let mut post = 0f64;
        let k = WHITEN_SCALES.len() as f32;
        // ONE interleaved loop (2026-09-10): estimate, residual, then the update — PER SAMPLE, so every update sees the error the CURRENT taps make. The first whitener build computed all 240 residuals with the frame's starting taps and then applied 240 updates against those stale errors: every update pushed the same way, the effective step was 240× the intended one, and the taps ran to infinity within two seconds on every phone (and in the tests, where NaN slipped past the ERLE arithmetic).
        for (j, m) in mic.iter_mut().enumerate() {
            // ref[frame_pos + j − ir_start − k] = win[j + n − 1 − k]: h ascending pairs with the window reversed.
            let x = &win[j..j + n];
            let est: f32 = self.h.iter().rev().zip(x).map(|(&h, &r)| h * r).sum::<f32>() * ref_gain;
            if !est.is_finite() {
                // The taps are gone: this and every later frame pass raw; the engine disarms on the self-check.
                self.diverged = true;
                return;
            }
            let raw = *m as f32;
            let e = raw - est;
            pre += (raw * raw) as f64;
            post += (e * e) as f64;
            *m = e.clamp(-32768.0, 32767.0) as i16;
            e_hist.push(e);
            if adapting {
                if j > 0 {
                    power += xw[j + n - 1] * xw[j + n - 1] - xw[j - 1] * xw[j - 1];
                }
                // Whitened error at this sample from the raw error history (this sample and its lags).
                let i = WHITEN_SPAN + j;
                let ew: f32 = WHITEN_SCALES.iter().map(|&s| e_hist[i] - e_hist[i - s]).sum::<f32>() / k;
                let norm = (power.max(0.0) * g2).max(floor);
                let g = MU * ew * ref_gain / norm;
                if !g.is_finite() {
                    self.diverged = true;
                    return;
                }
                let xwj = &xw[j..j + n];
                for (h, &r) in self.h.iter_mut().rev().zip(xwj) {
                    *h += g * r;
                }
            }
        }
        if adapting {
            self.pre_e += pre;
            self.post_e += post;
            self.adapted_frames += 1;
            const ALPHA: f64 = 0.02;
            self.recent_pre += (pre - self.recent_pre) * ALPHA;
            self.recent_post += (post - self.recent_post) * ALPHA;
        }
        // Error history for the next frame's whitening (adapted or not — the signal is continuous either way).
        let keep = len.min(WHITEN_SPAN);
        self.err_hist.rotate_left(keep);
        self.err_hist[WHITEN_SPAN - keep..].copy_from_slice(&e_hist[e_hist.len() - keep..]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(len: usize, amp: f32, seed: u64) -> Vec<f32> {
        let mut s = seed;
        (0..len)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((s >> 33) as i64 % 20001 - 10000) as f32 / 10000.0 * amp
            })
            .collect()
    }

    /// A sparse "room": direct spike + two reflections inside the tap window.
    fn true_h(ir_start: usize) -> (usize, Vec<f32>) {
        let mut h = vec![0f32; TAPS];
        h[PRE] = 0.4;
        h[PRE + 300] = -0.15;
        h[PRE + 900] = 0.08;
        (ir_start, h)
    }

    /// Build (ring, mic frames) where mic = ref convolved with true_h, mic timeline aligned so frame_pos maps identically.
    fn synth(frames: usize, seed: u64) -> (RefRing, Vec<Vec<i16>>, usize) {
        let flen = 240usize;
        let total = frames * flen + TAPS + 4096;
        let r = noise(total, 8000.0, seed);
        let mut ring = RefRing::new(total * 2);
        for c in r.chunks(flen) {
            let f: Vec<i16> = c.iter().map(|&v| v as i16).collect();
            ring.push(&f);
        }
        let ir_start = 1000usize;
        let (_, h) = true_h(ir_start);
        let mut mics = Vec::new();
        for fi in 0..frames {
            let base = TAPS + 4096 + fi * flen; // leave history so every window is resident
            let mut m = vec![0i16; flen];
            for j in 0..flen {
                let pos = base + j;
                let mut acc = 0f32;
                for (k, &hk) in h.iter().enumerate() {
                    let idx = pos as i64 - ir_start as i64 - k as i64;
                    if idx >= 0 && (idx as usize) < r.len() {
                        acc += hk * r[idx as usize];
                    }
                }
                m[j] = acc.clamp(-32768.0, 32767.0) as i16;
            }
            mics.push(m);
        }
        (ring, mics, ir_start)
    }

    #[test]
    fn seeded_filter_cancels_instantly() {
        let (ring, mics, ir_start) = synth(20, 3);
        let (_, h) = true_h(ir_start);
        let mut nlms = Nlms::new(ir_start, h);
        for (fi, mut m) in mics.into_iter().enumerate() {
            let pos = (TAPS + 4096 + fi * 240) as i64;
            nlms.cancel_frame(&mut m, &ring, pos, true, 1.0);
        }
        let erle = nlms.erle_db().expect("adapted");
        assert!(erle > 30.0, "seeded ERLE {erle:.1}dB — the chirp profile must cancel on frame one");
    }

    #[test]
    fn adaptation_converges_from_a_blunt_seed() {
        let (ring, mics, ir_start) = synth(400, 7);
        // Seed = direct spike only, half the true gain — reflections unlearned (a coarse chirp fit).
        let mut h = vec![0f32; TAPS];
        h[PRE] = 0.2;
        let mut nlms = Nlms::new(ir_start, h);
        let total = mics.len();
        let mut late_pre = 0f64;
        let mut late_post = 0f64;
        for (fi, mut m) in mics.into_iter().enumerate() {
            let pos = (TAPS + 4096 + fi * 240) as i64;
            let (p0, q0) = (nlms.pre_e, nlms.post_e);
            nlms.cancel_frame(&mut m, &ring, pos, true, 1.0);
            if fi >= total - 50 {
                late_pre += nlms.pre_e - p0;
                late_post += nlms.post_e - q0;
            }
        }
        assert!(nlms.h.iter().all(|v| v.is_finite()) && !nlms.diverged, "taps must stay finite");
        assert!(late_post.is_finite() && late_post > 0.0, "late residual must be a finite positive energy, got {late_post}");
        let late_erle = 10.0 * (late_pre / late_post).log10();
        assert!(late_erle > 20.0, "converged ERLE {late_erle:.1}dB after 2s of far-talk");
    }

    /// The whitener kills DC and passes a high tone: the shape that flattens speech's tilt.
    #[test]
    fn whitener_kills_dc_and_keeps_treble() {
        let dc: Vec<f32> = vec![1000.0; WHITEN_SPAN + 64];
        assert!(whiten(&dc).iter().all(|v| v.abs() < 1e-3));
        let hi: Vec<f32> = (0..WHITEN_SPAN + 64).map(|i| if i % 2 == 0 { 1000.0 } else { -1000.0 }).collect();
        let out = whiten(&hi);
        assert!(out.iter().map(|v| v.abs()).fold(0.0, f32::max) > 600.0, "a Nyquist tone must come thru the whitener near full (the even-lag differences vanish there, so two thirds)");
    }

    /// COLOURED input (a one-pole lowpass over noise, speech's tilt): a blunt seed must still converge — the whitened update is what makes this reachable in two seconds.
    #[test]
    fn adaptation_converges_on_coloured_input() {
        let flen = 240usize;
        let frames = 400usize;
        let total = frames * flen + TAPS + 4096;
        let white = noise(total, 8000.0, 11);
        let mut r = vec![0f32; total];
        let mut acc = 0f32;
        for i in 0..total {
            acc += (white[i] - acc) * 0.08; // ~600Hz one-pole lowpass at 48k
            r[i] = acc;
        }
        let mut ring = RefRing::new(total * 2);
        for c in r.chunks(flen) {
            let f: Vec<i16> = c.iter().map(|&v| v as i16).collect();
            ring.push(&f);
        }
        let ir_start = 1000usize;
        let (_, h_true) = true_h(ir_start);
        let mut h = vec![0f32; TAPS];
        h[PRE] = 0.2;
        let mut nlms = Nlms::new(ir_start, h);
        let (mut late_pre, mut late_post) = (0f64, 0f64);
        for fi in 0..frames {
            let base = TAPS + 4096 + fi * flen;
            let mut m = vec![0i16; flen];
            for j in 0..flen {
                let pos = base + j;
                let mut a = 0f32;
                for (k, &hk) in h_true.iter().enumerate() {
                    let idx = pos as i64 - ir_start as i64 - k as i64;
                    if idx >= 0 && (idx as usize) < r.len() {
                        a += hk * r[idx as usize];
                    }
                }
                m[j] = a.clamp(-32768.0, 32767.0) as i16;
            }
            let (p0, q0) = (nlms.pre_e, nlms.post_e);
            nlms.cancel_frame(&mut m, &ring, base as i64, true, 1.0);
            if fi >= frames - 50 {
                late_pre += nlms.pre_e - p0;
                late_post += nlms.post_e - q0;
            }
        }
        assert!(nlms.h.iter().all(|v| v.is_finite()) && !nlms.diverged, "taps must stay finite (diverged {}, adapted {})", nlms.diverged, nlms.adapted_frames);
        assert!(late_post.is_finite() && late_post > 0.0, "late residual must be a finite positive energy, got {late_post}");
        let late_erle = 10.0 * (late_pre / late_post).log10();
        assert!(late_erle > 12.0, "coloured-input ERLE {late_erle:.1}dB after 2s — the whitener must carry the tilt");
    }

    /// A SILENT reference with a loud near mic thru an open gate must not move the taps (the 2026-09-10 blow-up): the power floor refuses the update.
    #[test]
    fn silent_reference_never_poisons_the_taps() {
        let flen = 240usize;
        let total = 40 * flen + TAPS + 4096;
        let mut ring = RefRing::new(total * 2);
        for _ in 0..(total / flen) {
            ring.push(&vec![0i16; flen]);
        }
        let ir_start = 1000usize;
        let (_, h0) = true_h(ir_start);
        let mut nlms = Nlms::new(ir_start, h0.clone());
        for fi in 0..40 {
            let mut m: Vec<i16> = (0..flen).map(|i| if i % 2 == 0 { 12000 } else { -12000 }).collect();
            nlms.cancel_frame(&mut m, &ring, (TAPS + 4096 + fi * flen) as i64, true, 1.0);
            assert!(m.iter().all(|&v| v == 12000 || v == -12000), "raw must pass thru untouched over a silent reference");
        }
        assert!(!nlms.diverged);
        assert!(nlms.h.iter().zip(&h0).all(|(a, b)| (a - b).abs() < 1e-6), "taps moved on a silent reference");
    }

    #[test]
    fn ref_gain_tracks_a_volume_step_instantly() {
        // Echo doubles (user cranked the knob) — the exact-seeded filter with ref_gain 2.0 must still cancel deep, adaptation frozen.
        let (ring, mics, ir_start) = synth(20, 19);
        let (_, h) = true_h(ir_start);
        let mut nlms = Nlms::new(ir_start, h);
        let mut pre = 0f64;
        let mut post = 0f64;
        for (fi, m) in mics.into_iter().enumerate() {
            let mut loud: Vec<i16> = m.iter().map(|&s| (s as i32 * 2).clamp(-32768, 32767) as i16).collect();
            let pos = (TAPS + 4096 + fi * 240) as i64;
            let p: f64 = loud.iter().map(|&s| (s as f64) * (s as f64)).sum();
            nlms.cancel_frame(&mut loud, &ring, pos, false, 2.0);
            let q: f64 = loud.iter().map(|&s| (s as f64) * (s as f64)).sum();
            pre += p;
            post += q;
        }
        let erle = 10.0 * (pre / post.max(1e-9)).log10();
        assert!(erle > 25.0, "volume-stepped ERLE {erle:.1}dB");
    }

    #[test]
    fn frozen_filter_never_drifts_and_missing_reference_passes_thru() {
        let (ring, mics, ir_start) = synth(5, 11);
        let (_, h) = true_h(ir_start);
        let before = h.clone();
        let mut nlms = Nlms::new(ir_start, h);
        for (fi, mut m) in mics.into_iter().enumerate() {
            let pos = (TAPS + 4096 + fi * 240) as i64;
            nlms.cancel_frame(&mut m, &ring, pos, false, 1.0); // double-talk: frozen
        }
        assert_eq!(nlms.h, before, "frozen adaptation must not touch taps");
        assert_eq!(nlms.adapted_frames, 0);
        // A frame whose reference has aged out passes thru untouched.
        let mut m = vec![123i16; 240];
        let keep = m.clone();
        nlms.cancel_frame(&mut m, &ring, -100_000, true, 1.0);
        assert_eq!(m, keep);
    }
}
