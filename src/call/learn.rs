//! The in-call passive calibration learner (Nick's insight 2026-09-03): a running call already contains both Wave measurements — far-end-talking-alone stretches measure the speaker→mic coupling (we KNOW the far signal, we rendered it), near-end-talking-alone stretches measure the user's natural voice level. This module is the PURE math: streams in, estimates out, no platform or engine dependency — the KATs below are the proof the design demanded.
//!
//! **Model** per 10ms bin, far-active: `mic[t] ≈ g·far[t−d] + floor + speech[t]`, all envelopes ≥ 0 (mean |sample|; linear when `g·far ≳ 2·floor`).
//!
//! **The load-bearing decision — FLOOR-SUBTRACTION, NEVER MEAN-CENTERING**: subtracting the floor estimate keeps near-speech contamination one-sided (`Σfar·speech ≥ 0`, a contaminated window only OVER-estimates g), which is what makes the min-statistics low quantile converge to the clean-window value. Mean-centering removes the DC exactly but makes contamination two-sided, and the quantile then UNDER-estimates — the peer hears echo, the one failure this whole design exists to prevent. The uncorrected bias is `floor/(p̄(1+cv²))` (p̄ = far mean, cv = its coefficient of variation) — `kat_floor_bias` executes the derivation.
//!
//! **Separation invariant** (no feedback loop): the learner observes ONLY the pre-gain pre-duck mic envelope, its own binned render envelope, and vol/route. It never reads duck_gain, gate decisions, or the peak-hold far_level().

use super::calibrate::quietest_run;

/// 10ms of eagle oscillations — the bin width everything aligns on.
pub const BIN_OSC: i64 = (vsf::OSCILLATIONS_PER_SECOND / 100) as i64;
/// Estimator window: 3s (300 bins); bt:* routes use 4s so a 150-bin lag scan still leaves 2.5s of correlation.
const WINDOW_BINS: usize = 300;
const WINDOW_BINS_BT: usize = 400;
/// Lag scan cap: 1s covers every wired/builtin path; bt:* gets 1.5s (bad A2DP stacks).
const MAX_LAG: usize = 100;
const MAX_LAG_BT: usize = 150;
/// A far bin counts as ACTIVE above this envelope (quiet playback below it can't excite a measurable echo). Public: the predictive duck uses the same activity notion.
pub const FAR_ACT: f32 = 200.0;
/// Window acceptance: at least this many far-active bins (1s) and far mean > 4×floor (below that the envelope non-additivity breaks the linear model).
const WIN_FAR_ACTIVE_MIN: usize = 100;
/// Quality gate: Pearson r of the floor-subtracted pair at the peak lag — the PRIMARY double-talk defense (constant double-talk → low r → empty pool → never publishes).
const R_MIN: f32 = 0.5;
/// Reject a window whose lag lands in the top bins of the scan (peak-at-edge = true delay likely beyond the cap).
const EDGE_BINS: usize = 2;
/// Publish-time lag cluster: g publishes only when the low-g half's lags agree (this fraction within ±2 bins of their median). THE alias defense — bursty speech envelopes are pseudo-periodic, so when the TRUE delay sits outside the scan a sidelobe inside it passes the per-window gates with a plausible g (kat_delay_edge caught it); but a true delay is CONSTANT across windows while alias peaks wander with burst alignment. (A peak-to-sidelobe gate was tried first and failed the other way: long speech bursts make the true peak a PLATEAU, PSR ≈ 1.05, and it rejected clean windows wholesale.)
const CLUSTER_TOL: i64 = 2;
const CLUSTER_FRAC: f32 = 0.6;
/// The estimate pool: last N accepted windows; g = 25th percentile (min-statistics — needs ≥25% clean windows, which the r-gate guarantees), delay = median.
const POOL: usize = 30;
/// Confidence tiers: "usable" ducks in-call with wide margins; "solid" is the only tier persisted.
const USABLE_N: usize = 10;
const USABLE_SPREAD: f32 = 1.0; // IQR/median
const SOLID_N: usize = 20;
const SOLID_SPREAD: f32 = 0.5;
/// Delay lock: ≥5 pooled lags within ±1 bin of the median → lock and narrow the scan to ±3 bins (full rescan on route change or 3 consecutive edge pins).
const LOCK_AGREE: usize = 5;
const LOCK_SCAN: usize = 3;
/// Floor/voice guard: mic bins count as far-quiet only after the delayed far env has been quiet this long (echo tail can't contaminate the floor or the voice pool).
const FAR_QUIET_GUARD: usize = 30; // 300ms
/// Voice: voiced bins needed before talk publishes (5s), and the voiced threshold above floor (matches the ritual's).
const VOICED_MIN_BINS: usize = 500;
/// Shift detector: median of the last 5 windows > 2× the pooled 25th percentile → the physics changed (desktop volume knob, unobservable) → flush and re-converge.
const SHIFT_RECENT: usize = 5;
const SHIFT_FACTOR: f32 = 2.0;
/// Stamp regularizer: re-anchor when a stamp lands ≥40ms off the expected lattice — strictly deeper than a 4-frame callback burst, whose last frame deviates exactly −3 bins (a 3-bin threshold sat ON that boundary and collided the burst tail into the burst head).
const REANCHOR_OSC: i64 = BIN_OSC * 4;
const SLEW_OSC: i64 = BIN_OSC / 10;
/// Blend: EMA weight n/(n+N0), n capped — an ancient accumulation can't make the profile immovable.
const BLEND_N0: f32 = 20.0;
const BLEND_N_CAP: f32 = 100.0;

/// How much the pool trusts its estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
pub enum Confidence {
    None,
    Usable,
    Solid,
}

/// The learner's published state — everything the duck, the teardown log, and the persist path read.
#[derive(Debug, Clone)]
pub struct Estimate {
    /// Coupling gain, volume-normalized (÷ vol_lin at each window). None until the pool speaks.
    pub g_norm: Option<f32>,
    /// Render→capture delay in bins (10ms each). None until pooled.
    pub delay_bins: Option<usize>,
    pub confidence: Confidence,
    /// Accepted windows in the pool (the sample count the blend weighs by).
    pub windows: usize,
    /// Live noise-floor estimate (mean |sample| per frame).
    pub floor: f32,
    /// Natural talk level (trimmed median of voiced far-quiet bins). None until 5s of voiced bins.
    pub talk: Option<f32>,
    pub voiced_bins: usize,
    /// Per-gate window rejections [short, skew, hole, inactive, quiet, xcorr, reserved, edge, badg, r] — the teardown telemetry that convicts a starved pool instead of guessing.
    pub rejects: [u32; 10],
}

/// Turns bursty callback stamps (several frames sharing one osc — verified on desktop cpal) into a smooth 10ms lattice per stream, without frame-count drift (crystals differ ±100ppm ≈ 18 bins over 30min; stamps don't drift, counts do).
struct StampReg {
    expected: Option<i64>,
    /// Max deviation (now − expected) over the current slew window. MAX, not mean/median: burst frames 2..n arrive "early" against the lattice (they share the burst-start stamp), so mean/median slewing drags the lattice backward — the burst's FIRST frame is the one carrying the true cadence, and it is always the maximum deviation (kat_stamp_regularizer caught the per-frame version losing bins).
    dev_max: i64,
    dev_n: u32,
}

const SLEW_WINDOW: u32 = 16;

impl StampReg {
    fn new() -> Self {
        Self { expected: None, dev_max: i64::MIN, dev_n: 0 }
    }
    /// The regularized stamp for a frame arriving at `now`.
    fn stamp(&mut self, now: i64) -> i64 {
        match self.expected {
            Some(exp) if (now - exp).abs() < REANCHOR_OSC => {
                self.dev_max = self.dev_max.max(now - exp);
                self.dev_n += 1;
                let mut next = exp + BIN_OSC;
                if self.dev_n >= SLEW_WINDOW {
                    next += (self.dev_max / 8).clamp(-SLEW_OSC * SLEW_WINDOW as i64, SLEW_OSC * SLEW_WINDOW as i64);
                    self.dev_max = i64::MIN;
                    self.dev_n = 0;
                }
                self.expected = Some(next);
                exp
            }
            _ => {
                // Start/stop/stall: anchor the lattice here.
                self.expected = Some(now + BIN_OSC);
                self.dev_max = i64::MIN;
                self.dev_n = 0;
                now
            }
        }
    }
}

/// One stream's binned envelope history: absolute bin index (osc/BIN_OSC) → env. Holes (engine stall, hygiene clear) are NaN — a window containing mic holes fails acceptance; far holes are silence 0.0 (nothing rendered IS the far signal).
struct BinStream {
    start_bin: i64,
    vals: std::collections::VecDeque<f32>,
    reg: StampReg,
    hole_fill: f32,
    cap: usize,
}

impl BinStream {
    fn new(hole_fill: f32, cap: usize) -> Self {
        Self { start_bin: 0, vals: std::collections::VecDeque::new(), reg: StampReg::new(), hole_fill, cap }
    }
    fn push(&mut self, osc: i64, env: f32) {
        let bin = self.reg.stamp(osc) / BIN_OSC;
        if self.vals.is_empty() {
            self.start_bin = bin;
            self.vals.push_back(env);
        } else {
            let end = self.start_bin + self.vals.len() as i64;
            if bin < end {
                // Same-bin burst (regularizer re-anchor collision): blend rather than clobber.
                if bin >= self.start_bin {
                    let i = (bin - self.start_bin) as usize;
                    let old = self.vals[i];
                    self.vals[i] = if old.is_nan() { env } else { (old + env) * 0.5 };
                }
            } else {
                for _ in end..bin {
                    self.vals.push_back(self.hole_fill);
                }
                self.vals.push_back(env);
            }
        }
        while self.vals.len() > self.cap {
            self.vals.pop_front();
            self.start_bin += 1;
        }
    }
    /// The last `n` bins ending at this stream's newest bin, as (first_bin_index, values).
    fn tail(&self, n: usize) -> (i64, Vec<f32>) {
        let take = n.min(self.vals.len());
        let start = self.vals.len() - take;
        (self.start_bin + start as i64, self.vals.iter().skip(start).cloned().collect())
    }
}

/// One accepted window's verdict.
#[derive(Debug, Clone, Copy)]
struct WindowEstimate {
    lag: usize,
    g: f32,
}

pub struct Learner {
    far: BinStream,
    mic: BinStream,
    pool: std::collections::VecDeque<WindowEstimate>,
    /// Recent estimates (accepted or not-yet-pooled) for the shift detector.
    recent: std::collections::VecDeque<f32>,
    floor: f32,
    floor_frozen: bool,
    /// Bins since the delayed far env was last active — the far-quiet guard counter.
    far_quiet_run: usize,
    /// Voiced far-quiet mic bins (the voice pool), bounded.
    voiced: Vec<f32>,
    delay_lock: Option<usize>,
    edge_pins: usize,
    window_bins: usize,
    max_lag: usize,
    last_tick_bin: i64,
    /// Rejection tally per gate, for the teardown telemetry: [short, skew, hole, inactive, quiet, xcorr, lag_lo, edge, badg, r]. A field log that says WHICH gate starved the pool convicts; a bare "no estimate" guesses.
    pub rejects: [u32; 10],
}

impl Learner {
    /// `bt_route` widens the window + lag scan; `seed_delay_bins`/`seed_floor` come from a stored ritual profile when one exists.
    pub fn new(bt_route: bool, seed_delay_bins: Option<usize>, seed_floor: Option<f32>) -> Self {
        let window_bins = if bt_route { WINDOW_BINS_BT } else { WINDOW_BINS };
        let max_lag = if bt_route { MAX_LAG_BT } else { MAX_LAG };
        Self {
            far: BinStream::new(0.0, window_bins + max_lag + 64),
            mic: BinStream::new(f32::NAN, window_bins + max_lag + 64),
            pool: std::collections::VecDeque::new(),
            recent: std::collections::VecDeque::new(),
            floor: seed_floor.unwrap_or(40.0),
            floor_frozen: false,
            far_quiet_run: 0,
            voiced: Vec::new(),
            delay_lock: seed_delay_bins,
            edge_pins: 0,
            window_bins,
            max_lag,
            // 0, not an i64::MIN sentinel: eagle bins are huge positives, so 0 always fires the first tick — and MIN made the cadence subtraction WRAP negative (overflow-checks off), which returned early before ever setting the field: a permanently dead estimator (caught by the reject counters reading all-zero).
            last_tick_bin: 0,
            rejects: [0; 10],
        }
    }

    /// Feed one rendered-envelope entry (from the platform RENDER_ENV tap).
    pub fn push_far(&mut self, osc: i64, env: f32) {
        self.far.push(osc, env);
    }

    /// Feed one RAW mic envelope (pre-gain, pre-duck — the separation invariant) stamped at drain time. Also advances the far-quiet guard and the voice/floor pools, which work per-bin, not per-window.
    pub fn push_mic(&mut self, osc: i64, env: f32) {
        self.mic.push(osc, env);
        // The guard consults the DELAYED far env: the bin `delay` back from the newest far bin.
        let delay = self.delay_lock.unwrap_or(0);
        let far_now = {
            let n = self.far.vals.len();
            if n > delay { self.far.vals[n - 1 - delay] } else { 0.0 }
        };
        if far_now > FAR_ACT {
            self.far_quiet_run = 0;
        } else {
            self.far_quiet_run += 1;
        }
        if self.far_quiet_run >= FAR_QUIET_GUARD {
            // Far provably quiet: this mic bin is the user's room — floor and voice both learn from it.
            if env > self.floor * 4.0 + 40.0 {
                if self.voiced.len() < 6000 {
                    self.voiced.push(env);
                }
            }
        }
    }

    /// Run the estimator if a second has passed (bin time, not wall time — deterministic in KATs). `vol_lin` = linear volume factor (1.0 when unknown/desktop).
    pub fn tick(&mut self, vol_lin: f32) {
        let now_bin = self.far.start_bin + self.far.vals.len() as i64;
        if now_bin - self.last_tick_bin < 100 {
            return;
        }
        self.last_tick_bin = now_bin;
        self.update_floor();
        let Some(w) = self.window_estimate(vol_lin, true) else {
            return;
        };
        // Shift detector runs on ALL estimates (even quality-rejected ones would mask a shift — but a rejected estimate's g is untrustworthy, so recent tracks accepted only).
        self.recent.push_back(w.g);
        while self.recent.len() > SHIFT_RECENT {
            self.recent.pop_front();
        }
        if let Some(g25) = self.pool_g() {
            if self.recent.len() == SHIFT_RECENT {
                let mut r: Vec<f32> = self.recent.iter().cloned().collect();
                r.sort_by(|a, b| a.partial_cmp(b).unwrap());
                if r[SHIFT_RECENT / 2] > g25 * SHIFT_FACTOR {
                    // The physics changed (volume knob, case, distance): the old pool describes a dead regime.
                    self.pool.clear();
                    self.recent.clear();
                }
            }
        }
        self.pool.push_back(w);
        while self.pool.len() > POOL {
            self.pool.pop_front();
        }
        // Delay lock management: enough agreeing lags → lock + narrow scan; repeated edge pins → drop the lock and rescan wide.
        if let Some(med) = self.pool_delay() {
            let agree = self.pool.iter().filter(|e| (e.lag as i64 - med as i64).abs() <= 1).count();
            if agree >= LOCK_AGREE {
                self.delay_lock = Some(med);
            }
        }
    }

    /// Floor: quietest 300ms run over the mic tail, ONLY while the far-quiet guard holds broadly — an echo-contaminated floor over-subtracts and under-ducks the peer (the expensive direction). With no recent far-quiet stretch the floor freezes at its prior.
    fn update_floor(&mut self) {
        // Cheap conservatism: require the CURRENT guard run to be long enough that the tail's quietest run is provably far-free.
        if self.far_quiet_run >= FAR_QUIET_GUARD * 2 {
            let (_, mic) = self.mic.tail(self.window_bins);
            let clean: Vec<f32> = mic.iter().cloned().filter(|v| !v.is_nan()).collect();
            if clean.len() >= 60 {
                let f = quietest_run(&clean, 30);
                if f.is_finite() && f >= 0.0 {
                    // MINIMUM-STATISTICS: fall instantly, rise slowly (5%/tick ≈ 20s to a genuinely louder room). A user talking straight thru a far-quiet stretch makes quietest_run return TALK level — snapped in directly, that inflated floor rejected every window at the quiet gate (probe: rejects[4]=52) and over-subtraction under-ducks the peer. Speech is intermittent; rooms change slowly; the asymmetry encodes exactly that.
                    if f < self.floor {
                        self.floor = f;
                    } else {
                        self.floor += (f - self.floor) * 0.05;
                    }
                    self.floor_frozen = false;
                }
            }
        } else {
            self.floor_frozen = true;
        }
    }

    /// One window's (lag, g): floor-subtracted uncentered least-squares xcorr + acceptance/quality gates. `corrected=false` exists ONLY for the executable bias derivation in the KATs.
    fn window_estimate(&mut self, vol_lin: f32, corrected: bool) -> Option<WindowEstimate> {
        let need = self.window_bins;
        let (far_start, far) = self.far.tail(need);
        let (mic_start, mic) = self.mic.tail(need);
        if far.len() < need || mic.len() < need {
            self.rejects[0] += 1;
            return None;
        }
        // Align: both tails must cover the same bin range (streams run independently; a skewed tail = one stream stalled).
        if (far_start - mic_start).abs() > 2 {
            self.rejects[1] += 1;
            return None;
        }
        if mic.iter().any(|v| v.is_nan()) {
            self.rejects[2] += 1;
            return None;
        }
        // Acceptance: enough far excitation, loud enough over the floor for the linear model.
        let active = far.iter().filter(|&&v| v > FAR_ACT).count();
        if active < WIN_FAR_ACTIVE_MIN {
            self.rejects[3] += 1;
            return None;
        }
        let p_mean = far.iter().sum::<f32>() / far.len() as f32;
        if p_mean <= self.floor * 4.0 {
            self.rejects[4] += 1;
            return None;
        }
        let micp: Vec<f32> = if corrected {
            mic.iter().map(|&m| (m - self.floor).max(0.0)).collect()
        } else {
            mic.clone()
        };
        // Lag scan: ALWAYS wide. An earlier design narrowed the scan around the delay lock — and a lock formed from contaminated windows then rejected every CLEAN window while accepting the contaminated ones (kat_min_statistics caught the poisoning: pool quartile 0.67 vs true 0.25). The full scan costs ~20k multiplies per window — nothing. The lock survives only as the published value the duck aligns by. The play slice is shortened by the scan span (the envelope_xcorr length-asymmetry fix).
        let span = self.max_lag;
        let play = &far[..need - span.min(need - 1)];
        let Some((lag, g)) = xcorr(play, &micp, span) else {
            self.rejects[5] += 1;
            return None;
        };
        // Edge pin: the true delay likely sits beyond the scan — reject, and count toward dropping a stale lock.
        if lag + EDGE_BINS > span {
            self.edge_pins += 1;
            if self.edge_pins >= 3 {
                self.delay_lock = None;
                self.edge_pins = 0;
            }
            self.rejects[7] += 1;
            return None;
        }
        self.edge_pins = 0;
        if !g.is_finite() || g < 0.0 {
            self.rejects[8] += 1;
            return None;
        }
        // Quality gate: Pearson r at the peak — the primary double-talk defense.
        let r = pearson(play, &micp[lag..lag + play.len()]);
        if r < R_MIN {
            self.rejects[9] += 1;
            return None;
        }
        Some(WindowEstimate { lag, g: g / vol_lin.max(0.05) })
    }

    /// The published g — 25th percentile, gated on the lag CLUSTER: aliased pools (true delay outside the scan) carry plausible g values at wandering lags, and the wander is their tell.
    fn pool_g(&self) -> Option<f32> {
        if self.pool.is_empty() || !self.lags_cluster() {
            return None;
        }
        let mut gs: Vec<f32> = self.pool.iter().map(|e| e.g).collect();
        gs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        Some(gs[gs.len() / 4])
    }

    /// Do the low-g half's lags agree? (≥ CLUSTER_FRAC within ±CLUSTER_TOL of their median.)
    fn lags_cluster(&self) -> bool {
        let Some(med) = self.pool_delay() else {
            return false;
        };
        let mut by_g: Vec<&WindowEstimate> = self.pool.iter().collect();
        by_g.sort_by(|a, b| a.g.partial_cmp(&b.g).unwrap());
        let low = &by_g[..(by_g.len() / 2).max(1)];
        let agree = low.iter().filter(|e| (e.lag as i64 - med as i64).abs() <= CLUSTER_TOL).count();
        agree as f32 >= low.len() as f32 * CLUSTER_FRAC
    }

    /// Delay = median lag of the LOW-g half of the pool — min-statistics for the lag too: contaminated windows carry both inflated g AND unreliable lags, so the clean (low-g) half is the trustworthy voting bloc.
    fn pool_delay(&self) -> Option<usize> {
        if self.pool.is_empty() {
            return None;
        }
        let mut by_g: Vec<&WindowEstimate> = self.pool.iter().collect();
        by_g.sort_by(|a, b| a.g.partial_cmp(&b.g).unwrap());
        let low = &by_g[..(by_g.len() / 2).max(1)];
        let mut ls: Vec<usize> = low.iter().map(|e| e.lag).collect();
        ls.sort_unstable();
        Some(ls[ls.len() / 2])
    }

    fn pool_spread(&self) -> f32 {
        if self.pool.len() < 4 {
            return f32::MAX;
        }
        let mut gs: Vec<f32> = self.pool.iter().map(|e| e.g).collect();
        gs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = gs[gs.len() / 2].max(1e-6);
        let iqr = gs[gs.len() * 3 / 4] - gs[gs.len() / 4];
        iqr / med
    }

    /// The delay-aligned far envelope: the bin `delay_bins` back from the newest far bin — what the speaker emitted one acoustic round-trip ago, i.e. what the mic hears NOW. 0.0 when history is short (treated as far-silent: never gate on missing data).
    pub fn far_env_at(&self, delay_bins: usize) -> f32 {
        let n = self.far.vals.len();
        if n > delay_bins {
            let v = self.far.vals[n - 1 - delay_bins];
            if v.is_nan() { 0.0 } else { v }
        } else {
            0.0
        }
    }

    /// The published state — duck, teardown log, and persist all read this.
    pub fn estimate(&self) -> Estimate {
        let n = self.pool.len();
        let spread = self.pool_spread();
        let confidence = if n >= SOLID_N && spread < SOLID_SPREAD {
            Confidence::Solid
        } else if n >= USABLE_N && spread < USABLE_SPREAD {
            Confidence::Usable
        } else {
            Confidence::None
        };
        let talk = if self.voiced.len() >= VOICED_MIN_BINS {
            let mut v = self.voiced.clone();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let trim = v.len() / 8;
            let mid = &v[trim..v.len() - trim];
            Some(mid[mid.len() / 2])
        } else {
            None
        };
        Estimate {
            g_norm: self.pool_g(),
            delay_bins: self.pool_delay(),
            confidence,
            windows: n,
            floor: self.floor,
            talk,
            voiced_bins: self.voiced.len(),
            rejects: self.rejects,
        }
    }
}

/// Envelope cross-correlation: (lag, gain) at the peak; gain = Σcap·play/Σplay² (uncentered least-squares — see the module doc's floor-subtraction law). Mirrors calibrate::envelope_xcorr but lives here so the learner's contract can evolve without touching the ritual's.
fn xcorr(play: &[f32], cap: &[f32], max_lag: usize) -> Option<(usize, f32)> {
    if play.is_empty() || cap.len() < play.len() {
        return None;
    }
    let play_energy: f64 = play.iter().map(|&x| (x as f64) * (x as f64)).sum();
    if play_energy <= 0.0 {
        return None;
    }
    let top = max_lag.min(cap.len().saturating_sub(play.len()));
    let mut best = (0usize, f64::MIN);
    for lag in 0..=top {
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

/// The predictive duck's per-frame verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateVerdict {
    /// Far silent — full mic.
    Full,
    /// Far talking, mic ≈ predicted echo — hard gate (the near human is not talking).
    Gate,
    /// Far talking, mic ≫ prediction — double-talk, soft duck only.
    Duck,
}

/// The predictive gate with HYSTERESIS: enter the hard gate when mic < max(pred×2, floor×2), leave it only when mic > max(pred×3, floor×3) — without the band, a mic level riding the threshold chatters the gate open/shut every frame (audible stutter on the far end). Pure + stateful-in-one-bool, so the decision table and the chatter bound are KATs.
pub struct PredGate {
    gated: bool,
}

impl PredGate {
    pub fn new() -> Self {
        Self { gated: false }
    }
    pub fn decide(&mut self, mic_mean: f32, pred: f32, floor: f32, far_active: bool) -> GateVerdict {
        if !far_active {
            self.gated = false;
            return GateVerdict::Full;
        }
        let enter = (pred * 2.0).max(floor * 2.0);
        let exit = (pred * 3.0).max(floor * 3.0);
        if self.gated {
            if mic_mean > exit {
                self.gated = false;
            }
        } else if mic_mean < enter {
            self.gated = true;
        }
        if self.gated {
            GateVerdict::Gate
        } else {
            GateVerdict::Duck
        }
    }
}

/// Pearson correlation — the window quality gate's statistic.
fn pearson(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    if n < 2 {
        return 0.0;
    }
    let (ma, mb) = (
        a[..n].iter().sum::<f32>() / n as f32,
        b[..n].iter().sum::<f32>() / n as f32,
    );
    let (mut sab, mut saa, mut sbb) = (0f64, 0f64, 0f64);
    for i in 0..n {
        let (da, db) = ((a[i] - ma) as f64, (b[i] - mb) as f64);
        sab += da * db;
        saa += da * da;
        sbb += db * db;
    }
    if saa <= 0.0 || sbb <= 0.0 {
        return 0.0;
    }
    (sab / (saa.sqrt() * sbb.sqrt())) as f32
}

/// Blend a learned g into the stored profile. RITUAL OUTRANKS: a fresh ritual overwrite resets `stored_n` to 0 upstream. ASYMMETRIC: raising g (duck more) blends at full weight; lowering blends at ¼ weight and only from a Solid estimate — publishing too-low g inflicts echo on the peer. Returns (blended g, new sample count).
pub fn blend_g(stored: f32, stored_n: f32, learned: f32, learned_windows: usize, confidence: Confidence) -> (f32, f32) {
    let w = learned_windows as f32;
    let mut alpha = w / (w + stored_n + BLEND_N0);
    if learned < stored {
        if confidence != Confidence::Solid {
            return (stored, stored_n);
        }
        alpha *= 0.25;
    }
    let g = stored + alpha * (learned - stored);
    let n = (stored_n + w).min(BLEND_N_CAP);
    (g, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic xorshift — the KATs need noise, never nondeterminism.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % 1000) as f32 / 1000.0
        }
    }

    /// Speech-like far envelope: talk bursts (0.5–1.5s at ~2000±spread) separated by pauses (0.3–0.8s of near-zero).
    fn gen_far(bins: usize, rng: &mut Rng) -> Vec<f32> {
        let mut out = Vec::with_capacity(bins);
        while out.len() < bins {
            let burst = 50 + (rng.next() * 100.0) as usize;
            for _ in 0..burst {
                out.push(1500.0 + rng.next() * 1000.0);
            }
            let pause = 30 + (rng.next() * 50.0) as usize;
            for _ in 0..pause {
                out.push(rng.next() * 20.0);
            }
        }
        out.truncate(bins);
        out
    }

    /// mic = g·far_delayed + floor + noise (+ optional near-speech mask).
    fn gen_mic(far: &[f32], g: f32, delay: usize, floor: f32, rng: &mut Rng, near: Option<&[f32]>) -> Vec<f32> {
        (0..far.len())
            .map(|t| {
                let f = if t >= delay { far[t - delay] } else { 0.0 };
                let n = near.map_or(0.0, |sp| sp[t]);
                g * f + floor + rng.next() * floor * 0.3 + n
            })
            .collect()
    }

    /// Drive a Learner with regular 10ms stamps; ticks fire on bin cadence internally.
    fn run(l: &mut Learner, far: &[f32], mic: &[f32], vol: f32) {
        let t0 = 1_000_000_000i64;
        for i in 0..far.len() {
            let osc = t0 + i as i64 * BIN_OSC;
            l.push_far(osc, far[i]);
            l.push_mic(osc, mic[i]);
            if i % 100 == 99 {
                l.tick(vol);
            }
        }
    }

    /// Clean call: g and delay recovered within tolerance, confidence reaches Solid.
    #[test]
    fn kat_recover_g_and_delay() {
        let mut rng = Rng(42);
        let far = gen_far(4000, &mut rng); // 40s
        let mic = gen_mic(&far, 0.3, 17, 30.0, &mut rng, None);
        let mut l = Learner::new(false, None, None);
        run(&mut l, &far, &mic, 1.0);
        let e = l.estimate();
        let g = e.g_norm.expect("pool speaks");
        assert!((g - 0.3).abs() / 0.3 < 0.05, "g {g} vs 0.3 within 5%");
        assert_eq!(e.delay_bins, Some(17), "delay exact");
        assert_eq!(e.confidence, Confidence::Solid, "clean 40s call is solid (windows {})", e.windows);
    }

    /// The executable bias derivation: uncorrected LS over-estimates by ≈ floor/(p̄(1+cv²)); floor-subtraction removes it.
    #[test]
    fn kat_floor_bias_derivation() {
        let mut rng = Rng(7);
        let far = gen_far(3000, &mut rng);
        let g = 0.2f32;
        let floor = {
            let p_mean = far.iter().sum::<f32>() / far.len() as f32;
            0.5 * g * p_mean
        };
        let mic = gen_mic(&far, g, 10, floor, &mut rng, None);
        // Predicted bias from the derivation.
        let p_mean = far.iter().sum::<f32>() / far.len() as f32;
        let var = far.iter().map(|&p| (p - p_mean) * (p - p_mean)).sum::<f32>() / far.len() as f32;
        let cv2 = var / (p_mean * p_mean);
        let predicted_bias = floor / (p_mean * (1.0 + cv2));
        // Uncorrected: measure thru the private path with corrected=false.
        let mut lu = Learner::new(false, None, Some(floor));
        run(&mut lu, &far, &mic, 1.0);
        let (_, f) = lu.far.tail(lu.window_bins);
        let (_, m) = lu.mic.tail(lu.window_bins);
        let span = lu.max_lag;
        let play = &f[..lu.window_bins - span];
        let (_, g_unc) = xcorr(play, &m, span).unwrap();
        let bias = g_unc - g;
        assert!(
            (bias - predicted_bias).abs() / predicted_bias < 0.5,
            "measured bias {bias:.4} tracks the derivation {predicted_bias:.4}"
        );
        // Corrected: the learner's real path lands within 10%.
        let e = lu.estimate();
        let gc = e.g_norm.expect("pool speaks");
        assert!((gc - g).abs() / g < 0.10, "corrected g {gc} vs {g} within 10%");
    }

    /// Min-statistics under 50% double-talk: the mean is dragged up, the 25th percentile holds — and a mean-CENTERED estimator's quantile is dragged LOW (locks in the floor-subtraction decision forever).
    #[test]
    fn kat_min_statistics_under_double_talk() {
        let mut rng = Rng(1234);
        let far = gen_far(6000, &mut rng); // 60s
        let g = 0.25f32;
        // Near speech on ~half the timeline in 3s blocks (whole windows contaminated, others clean).
        let near: Vec<f32> = (0..far.len())
            .map(|t| if (t / 300) % 2 == 0 { 1200.0 + rng.next() * 800.0 } else { 0.0 })
            .collect();
        let mic = gen_mic(&far, g, 12, 30.0, &mut rng, Some(&near));
        let mut l = Learner::new(false, None, None);
        run(&mut l, &far, &mic, 1.0);
        let e = l.estimate();
        let g25 = e.g_norm.expect("clean windows exist — pool speaks");
        assert!((g25 - g).abs() / g < 0.15, "25th pct {g25} vs {g} within 15% despite 50% double-talk");
        // The pool's UPPER half carries the contamination (one-sided).
        let mut gs: Vec<f32> = l.pool.iter().map(|w| w.g).collect();
        gs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!(gs[gs.len() - 1] > g * 1.3, "contaminated windows over-estimate (max {})", gs[gs.len() - 1]);
        // Mean-centered variant (test-local): centering makes contamination two-sided; its low quantile under-estimates.
        let mut centered: Vec<f32> = Vec::new();
        for w0 in (0..far.len() - 300).step_by(100) {
            let fw = &far[w0..w0 + 200];
            let mw = &mic[w0 + 12..w0 + 12 + 200];
            let fm = fw.iter().sum::<f32>() / 200.0;
            let mm = mw.iter().sum::<f32>() / 200.0;
            let num: f32 = fw.iter().zip(mw).map(|(&p, &m)| (p - fm) * (m - mm)).sum();
            let den: f32 = fw.iter().map(|&p| (p - fm) * (p - fm)).sum();
            if den > 0.0 {
                centered.push(num / den);
            }
        }
        centered.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let c25 = centered[centered.len() / 4];
        assert!(c25 < g25, "centered 25th pct {c25} sits below floor-subtracted {g25} — centering under-estimates under double-talk");
    }

    /// Constant double-talk: the r-gate keeps the pool empty; the learner never publishes a lie.
    #[test]
    fn kat_constant_double_talk_never_publishes() {
        let mut rng = Rng(99);
        let far = gen_far(4000, &mut rng);
        let near: Vec<f32> = (0..far.len()).map(|_| 1500.0 + rng.next() * 1000.0).collect();
        let mic = gen_mic(&far, 0.05, 15, 30.0, &mut rng, Some(&near));
        let mut l = Learner::new(false, None, None);
        run(&mut l, &far, &mic, 1.0);
        let e = l.estimate();
        assert_eq!(e.confidence, Confidence::None, "constant double-talk must not reach usable (windows {})", e.windows);
        assert!(e.windows < USABLE_N, "the r-gate starves the pool (got {})", e.windows);
    }

    /// Delay beyond the scan cap is rejected (edge pin), and a BT-widened learner recovers a 400ms delay.
    #[test]
    fn kat_delay_edge_and_bt_window() {
        let mut rng = Rng(5);
        let far = gen_far(5000, &mut rng);
        let mic = gen_mic(&far, 0.3, 120, 30.0, &mut rng, None); // 1.2s delay
        let mut l = Learner::new(false, None, None); // narrow: cap 100 — must refuse
        run(&mut l, &far, &mic, 1.0);
        assert_eq!(l.estimate().g_norm, None, "delay past the scan cap must never yield an estimate");
        let mut lbt = Learner::new(true, None, None); // bt: cap 150
        let mic_bt = gen_mic(&far, 0.3, 40, 30.0, &mut rng, None); // 400ms
        run(&mut lbt, &far, &mic_bt, 1.0);
        let e = lbt.estimate();
        assert_eq!(e.delay_bins, Some(40), "BT window recovers a 400ms delay");
    }

    /// Burst stamps (4 frames sharing one osc) still land on a clean lattice, and ±100ppm cadence drift over 30min binds < 1 bin of error.
    #[test]
    fn kat_stamp_regularizer() {
        let mut s = BinStream::new(0.0, 200_000);
        let t0 = 5_000_000_000i64;
        // 4-frame bursts every 40ms for 1000 frames.
        for burst in 0..250i64 {
            let osc = t0 + burst * 4 * BIN_OSC;
            for k in 0..4 {
                s.push(osc, (burst * 4 + k) as f32);
            }
        }
        assert_eq!(s.vals.len(), 1000, "burst stamps regularize onto the 10ms lattice, one bin per frame");
        // ±100ppm drift: frames arrive every BIN_OSC×1.0001 for 30min (180k frames).
        let mut d = BinStream::new(0.0, 200_000);
        let step = BIN_OSC + BIN_OSC / 10_000;
        for i in 0..180_000i64 {
            d.push(t0 + i * step, 1.0);
        }
        let extra = d.vals.len() as i64 - 180_000;
        assert!(extra.abs() <= 20, "±100ppm over 30min stays within the slew's tracking (extra bins {extra})");
        assert!(d.vals.iter().filter(|v| **v == 0.0).count() <= 20, "no hole cascade under drift");
    }

    /// A g step (volume knob) flushes the pool and re-converges to the new physics.
    #[test]
    fn kat_shift_flush_and_reconverge() {
        let mut rng = Rng(77);
        let far = gen_far(8000, &mut rng); // 80s
        let mut mic = gen_mic(&far[..4000], 0.15, 14, 30.0, &mut rng, None);
        mic.extend(gen_mic(&far[4000..], 0.45, 14, 30.0, &mut rng, None)); // knob turned: g×3
        let mut l = Learner::new(false, None, None);
        run(&mut l, &far, &mic, 1.0);
        let e = l.estimate();
        let g = e.g_norm.expect("post-shift pool speaks");
        assert!((g - 0.45).abs() / 0.45 < 0.15, "re-converged to the new g (got {g})");
    }

    /// Voice: near-only speech over a quiet far end yields the talk level and the 4000/talk gain contract.
    #[test]
    fn kat_voice_level() {
        let mut rng = Rng(3);
        let far = vec![0.0f32; 3000]; // far silent — pure near-talk stretch
        let near: Vec<f32> = (0..3000).map(|t| if (t / 200) % 2 == 0 { 900.0 + rng.next() * 200.0 } else { 0.0 }).collect();
        let mic: Vec<f32> = near.iter().map(|&n| n + 25.0 + rng.next() * 8.0).collect();
        let mut l = Learner::new(false, None, None);
        run(&mut l, &far, &mic, 1.0);
        let e = l.estimate();
        let talk = e.talk.expect("5s+ of voiced bins");
        assert!((talk - 1000.0).abs() < 150.0, "talk ≈ speech median (got {talk})");
        assert!(e.floor < 100.0, "floor tracked the quiet gaps (got {})", e.floor);
    }

    /// Blend: EMA weights, the asymmetric lower rule, and the sample-count cap.
    #[test]
    fn kat_blend_policy() {
        // Raising g: full weight. stored 0.2/n=0, learned 0.4 over 20 windows → alpha 20/(20+0+20)=0.5 → 0.3.
        let (g, n) = blend_g(0.2, 0.0, 0.4, 20, Confidence::Usable);
        assert!((g - 0.3).abs() < 1e-6);
        assert_eq!(n, 20.0);
        // Lowering without Solid: refused outright.
        let (g, n) = blend_g(0.4, 20.0, 0.2, 20, Confidence::Usable);
        assert_eq!((g, n), (0.4, 20.0), "a non-solid lower must not move the profile");
        // Lowering with Solid: quarter weight. alpha = 20/(20+20+20)×0.25 = 0.0833… → 0.4 − 0.0167 ≈ 0.3833.
        let (g, _) = blend_g(0.4, 20.0, 0.2, 20, Confidence::Solid);
        assert!((g - (0.4 + (20.0 / 60.0) * 0.25 * (0.2 - 0.4))).abs() < 1e-6);
        // The count caps: an immovable profile is forbidden.
        let (_, n) = blend_g(0.3, 95.0, 0.35, 30, Confidence::Solid);
        assert_eq!(n, BLEND_N_CAP);
    }


    /// The predictive gate's decision table + the hysteresis chatter bound.
    #[test]
    fn kat_pred_gate_table_and_chatter() {
        let mut g = PredGate::new();
        // Far silent → Full, always, and it resets the latch.
        assert_eq!(g.decide(5000.0, 100.0, 30.0, false), GateVerdict::Full);
        // Far talking, mic at echo level (pred 100, mic 150 < enter 200) → Gate.
        assert_eq!(g.decide(150.0, 100.0, 30.0, true), GateVerdict::Gate);
        // Still gated at mic 250 (exit is 300 — inside the hysteresis band).
        assert_eq!(g.decide(250.0, 100.0, 30.0, true), GateVerdict::Gate);
        // Mic 400 > exit 300 → the near human is talking → Duck (double-talk).
        assert_eq!(g.decide(400.0, 100.0, 30.0, true), GateVerdict::Duck);
        // Floor dominates a tiny prediction: pred 5, floor 30 → enter 60.
        let mut g2 = PredGate::new();
        assert_eq!(g2.decide(40.0, 5.0, 30.0, true), GateVerdict::Gate);
        // Chatter bound: mic rides EXACTLY between the thresholds (pred 100: enter 200, exit 300; mic 250) — the verdict must be CONSTANT, not alternating.
        let mut g3 = PredGate::new();
        let first = g3.decide(250.0, 100.0, 30.0, true);
        let mut flips = 0;
        let mut last = first;
        for i in 0..1000 {
            // Wobble ±20 around 250 — still inside the band both ways.
            let mic = 250.0 + if i % 2 == 0 { 20.0 } else { -20.0 };
            let v = g3.decide(mic, 100.0, 30.0, true);
            if v != last {
                flips += 1;
                last = v;
            }
        }
        assert_eq!(flips, 0, "in-band wobble must never flip the verdict");
    }

    /// far_env_at: the delay-aligned lookup the prediction rides.
    #[test]
    fn kat_far_env_at() {
        let mut l = Learner::new(false, None, None);
        let t0 = 1_000_000_000i64;
        for i in 0..50i64 {
            l.push_far(t0 + i * BIN_OSC, i as f32);
        }
        assert_eq!(l.far_env_at(0), 49.0, "delay 0 = newest bin");
        assert_eq!(l.far_env_at(10), 39.0, "delay 10 bins back");
        assert_eq!(l.far_env_at(60), 0.0, "past history = far-silent, never a gate on missing data");
    }
}
