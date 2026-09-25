//! TrueClock (docs/lock.md §4): the model mapping photon's suspend-counting monotonic clock ("boot", in oscillations) to Eagle Time, with its uncertainty and lock source.
//! Pure — no clock reads, no globals — so every rule here is testable against synthetic exchanges; the process-wide instance and the clock reads live in `time_base`.
//! Integers name every instant; floats appear only inside the regression (the spec's rule: "floats are allowed only inside estimators").

use crate::OSC_PER_SEC;

/// What disciplines the clock (spec §1.4). `Peer` = relative lock to another photon device only; `Free` = never disciplined, stamps are not absolute time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LockSource {
    Gnss = 0,
    Ptp = 1,
    Ntp = 2,
    Peer = 3,
    Holdover = 4,
    Free = 5,
}

impl LockSource {
    /// The lock's name, for logs and the UI.
    pub fn name(self) -> &'static str {
        match self {
            LockSource::Gnss => "GNSS",
            LockSource::Ptp => "PTP",
            LockSource::Ntp => "NTP",
            LockSource::Peer => "peer",
            LockSource::Holdover => "holdover",
            LockSource::Free => "free",
        }
    }
}

/// A true-time reading: the Eagle count, how far it may be off, and what vouches for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub eagle: i64,
    pub uncertainty_ns: u32,
    pub source: LockSource,
}

impl Stamp {
    /// Spec §4.3: past 5 ms the grid-alignment guarantees no longer hold (the wave continues, marked degraded).
    pub fn degraded(&self) -> bool {
        self.uncertainty_ns > DEGRADED_NS || self.source == LockSource::Free
    }
}

/// One round-trip exchange against a reference, on the boot clock: at local instant `boot`, true time was `boot + offset`; `delay` is the round trip less the remote's hold (the path the offset could hide in).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exchange {
    pub boot: i64,
    pub offset: i64,
    pub delay: i64,
}

/// Uncertainty past which a stamp is degraded (spec §4.3).
pub const DEGRADED_NS: u32 = 5_000_000;
/// The window: at most this many exchanges (spec §4.2's 64) …
pub const WINDOW_MAX: usize = 64;
/// … and none older than this. The spec's 5 minutes assumes a continuous exchange stream; photon's reference is a periodic consensus, so the window is sized to hold several of them — which is also what lets the slope (the crystal's rate) be measured at all.
pub const WINDOW_AGE_OSC: i64 = 6 * 3600 * OSC_PER_SEC;
/// The kept exchanges must span at least this long before a rate is fitted; a single burst of queries says nothing about drift.
pub const MIN_RATE_SPAN_OSC: i64 = 60 * OSC_PER_SEC;
/// Rate uncertainty when the rate is unknown (spec §4.3: 2 ppm ⇒ 2 µs/s of holdover growth).
pub const DEFAULT_RATE_UNC_PPB: i64 = 2_000;
/// A fit older than this is holdover, not lock (a consensus is expected well inside it).
pub const HOLDOVER_AFTER_OSC: i64 = 2 * 3600 * OSC_PER_SEC;
/// While a wave is live the mapping never steps: a new fit's offset change is slewed in at this rate (spec §4.3: 50 µs per second).
pub const SLEW_NS_PER_SEC: i64 = 50_000;

/// The fitted map: true = ref_true + Δ + Δ·rate, with Δ = boot − ref_boot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model {
    pub ref_boot: i64,
    pub ref_true: i64,
    /// Parts per billion the boot clock runs SLOW against true time (positive ⇒ true advances faster).
    pub rate_ppb: i64,
    /// Uncertainty at the fit instant (ns), and how fast it grows away from it.
    pub unc_ns: u64,
    pub rate_unc_ppb: i64,
    /// Boot instant of the newest exchange the fit used — holdover ages from here.
    pub fit_boot: i64,
    pub source: LockSource,
}

impl Model {
    /// Map a boot instant to Eagle Time, in integers.
    pub fn eagle_at(&self, boot: i64) -> i64 {
        let d = (boot - self.ref_boot) as i128;
        (self.ref_true as i128 + d + d * self.rate_ppb as i128 / 1_000_000_000) as i64
    }

    /// The inverse, for scheduling: the boot instant at which true time reads `eagle`, solved in closed form (Δboot = Δtrue · 10⁹ / (10⁹ + rate)).
    /// Exact to one oscillation: a clock running slow maps neighbouring boot instants onto one Eagle count, so either is a right answer.
    pub fn boot_at(&self, eagle: i64) -> i64 {
        let dt = (eagle - self.ref_true) as i128;
        (self.ref_boot as i128 + dt * 1_000_000_000 / (1_000_000_000 + self.rate_ppb as i128)) as i64
    }

    /// Uncertainty at `boot`: the fit's own, grown by the rate uncertainty for every oscillation away from the newest exchange (holdover growth, spec §4.2).
    pub fn unc_ns_at(&self, boot: i64) -> u64 {
        let age_ns = (boot - self.fit_boot).unsigned_abs() as u128 * 1_000_000_000 / OSC_PER_SEC as u128;
        self.unc_ns + (age_ns * self.rate_unc_ppb.unsigned_abs() as u128 / 1_000_000_000) as u64
    }

    fn source_at(&self, boot: i64) -> LockSource {
        if self.source != LockSource::Free && boot - self.fit_boot > HOLDOVER_AFTER_OSC {
            LockSource::Holdover
        } else {
            self.source
        }
    }
}

fn osc_to_ns(osc: i64) -> u64 {
    (osc.unsigned_abs() as u128 * 1_000_000_000 / OSC_PER_SEC as u128) as u64
}

/// What one fit saw, for the log line the spec asks for on every fit (§4.3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FitReport {
    pub window: usize,
    pub kept: usize,
    pub min_delay_ns: u64,
    pub residual_ns: u64,
    pub rate_ppb: i64,
}

/// The minimum filter's bin: the spec's 5-minute window, applied per bin because photon's reference arrives in bursts (one consensus = tens of exchanges within seconds, hours apart).
/// A quartile over the whole window could take every kept point from one lucky burst and leave no span to fit a rate across; per bin, every burst contributes its cleanest exchanges.
pub const BIN_OSC: i64 = 5 * 60 * OSC_PER_SEC;

/// Fit a model to the window (spec §4.2): in each 5-minute bin keep the lowest-delay quartile (asymmetric queuing shows up as high delay), then a least-squares line of offset against boot time across all bins — the intercept is the offset, the slope the rate.
/// `None` for an empty window.
pub fn fit(window: &[Exchange], source: LockSource) -> Option<(Model, FitReport)> {
    if window.is_empty() {
        return None;
    }
    let mut sorted: Vec<Exchange> = window.to_vec();
    sorted.sort_unstable_by_key(|e| (e.boot.div_euclid(BIN_OSC), e.delay));
    let mut kept: Vec<Exchange> = Vec::new();
    for bin in sorted.chunk_by(|a, b| a.boot.div_euclid(BIN_OSC) == b.boot.div_euclid(BIN_OSC)) {
        // The bin's lowest-delay quartile, at least its single best exchange.
        kept.extend_from_slice(&bin[..(bin.len() / 4).max(1)]);
    }
    let keep = kept.len();
    let kept = &kept[..];
    let min_delay = kept.iter().map(|e| e.delay).min()?.max(0); // WHY/PROOF: a delay is round trip minus hold, which clock quantization can leave a hair negative — no path is shorter than instant
    let newest = kept.iter().map(|e| e.boot).max()?;
    let oldest = kept.iter().map(|e| e.boot).min()?;

    // Centre on the means so the regression works on small numbers (f64 keeps ~15 digits; raw oscillation counts have 19).
    let n = kept.len() as f64;
    let mb = kept.iter().map(|e| (e.boot - kept[0].boot) as f64).sum::<f64>() / n;
    let mo = kept.iter().map(|e| (e.offset - kept[0].offset) as f64).sum::<f64>() / n;
    let (mut sxx, mut sxy) = (0.0f64, 0.0f64);
    for e in kept {
        let x = (e.boot - kept[0].boot) as f64 - mb;
        let y = (e.offset - kept[0].offset) as f64 - mo;
        sxx += x * x;
        sxy += x * y;
    }
    let rated = newest - oldest >= MIN_RATE_SPAN_OSC && sxx > 0.0;
    let slope = if rated { sxy / sxx } else { 0.0 };
    let resid_var = kept
        .iter()
        .map(|e| {
            let x = (e.boot - kept[0].boot) as f64 - mb;
            let y = (e.offset - kept[0].offset) as f64 - mo;
            (y - slope * x).powi(2)
        })
        .sum::<f64>()
        / n;
    let resid_osc = resid_var.sqrt();
    // Rate uncertainty: the slope's standard error when there is a slope, else the spec's default.
    let rate_unc_ppb = if rated { ((resid_osc / sxx.sqrt()) * 1e9).ceil() as i64 + 1 } else { DEFAULT_RATE_UNC_PPB };

    let ref_boot = kept[0].boot + mb.round() as i64;
    let ref_offset = kept[0].offset + mo.round() as i64;
    let resid_ns = osc_to_ns(resid_osc.ceil() as i64);
    let min_delay_ns = osc_to_ns(min_delay);
    let unc_ns = (min_delay_ns / 2).max(3 * resid_ns).max(1);
    let rate_ppb = (slope * 1e9).round() as i64;
    let model = Model { ref_boot, ref_true: ref_boot + ref_offset, rate_ppb, unc_ns, rate_unc_ppb, fit_boot: newest, source };
    Some((model, FitReport { window: window.len(), kept: keep, min_delay_ns, residual_ns: resid_ns, rate_ppb }))
}

/// A slew in progress: output walks from `from` toward the current model at [`SLEW_NS_PER_SEC`], starting at `since` (boot).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Slew {
    from: Model,
    since: i64,
}

/// The boot → Eagle mapping without its uncertainty bookkeeping: `(ref_boot, ref_true, rate_ppb)` for the model and, while a slew is in flight, the model it slews from plus the boot instant it began.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub model: Option<(i64, i64, i64)>,
    pub slew: Option<(i64, i64, i64, i64)>,
}

impl Snapshot {
    /// True time at `boot`; `None` before any fit.
    pub fn eagle_at(&self, boot: i64) -> Option<i64> {
        let map = |(rb, rt, r): (i64, i64, i64)| {
            let d = (boot - rb) as i128;
            (rt as i128 + d + d * r as i128 / 1_000_000_000) as i64
        };
        let target = map(self.model?);
        let Some((fb, ft, fr, since)) = self.slew else { return Some(target) };
        let from = map((fb, ft, fr));
        let allowed = (boot - since).max(0) as i128 * SLEW_NS_PER_SEC as i128 / 1_000_000_000; // WHY/PROOF: `since` is a past boot instant, but a caller converting a PAST instant can ask about a moment before the slew began — nothing has slewed yet there
        let diff = (target - from) as i128;
        Some(if diff.abs() <= allowed { target } else { (from as i128 + allowed * diff.signum()) as i64 })
    }
}

/// The discipline state: the window, the current model, and a slew while a wave forbids steps.
#[derive(Clone, Debug, Default)]
pub struct TrueClock {
    window: Vec<Exchange>,
    model: Option<Model>,
    slew: Option<Slew>,
}

impl TrueClock {
    pub const fn new() -> Self {
        Self { window: Vec::new(), model: None, slew: None }
    }

    /// Feed exchanges measured up to `now_boot` and refit. `no_step` (a wave is live) turns a jump into a 50 µs/s slew; otherwise the new model applies at once.
    pub fn feed(&mut self, exchanges: &[Exchange], source: LockSource, now_boot: i64, no_step: bool) -> Option<FitReport> {
        self.window.extend_from_slice(exchanges);
        self.window.retain(|e| now_boot - e.boot <= WINDOW_AGE_OSC);
        if self.window.len() > WINDOW_MAX {
            let drop = self.window.len() - WINDOW_MAX;
            self.window.sort_unstable_by_key(|e| e.boot);
            self.window.drain(..drop);
        }
        let (model, report) = fit(&self.window, source)?;
        self.install(model, now_boot, no_step);
        Some(report)
    }

    /// Replace everything with one reference reading (a server that just proved our clock wrong by more than a window): the window restarts from it, and it steps — a clock that far off is not something to slew thru.
    pub fn reset_to(&mut self, exchange: Exchange, source: LockSource) {
        self.window.clear();
        self.window.push(exchange);
        self.slew = None;
        self.model = fit(&self.window, source).map(|(m, _)| m);
    }

    fn install(&mut self, model: Model, now_boot: i64, no_step: bool) {
        self.slew = match (self.model, no_step) {
            // Slew from wherever the output stands NOW (mid-slew included), so a second fit during a wave cannot step either.
            (Some(_), true) => Some(Slew { from: self.frozen_model_at(now_boot), since: now_boot }),
            _ => None,
        };
        self.model = Some(model);
    }

    /// A model that reproduces the current output at `boot` with the current model's rate — the start point of a new slew.
    fn frozen_model_at(&self, boot: i64) -> Model {
        let mut m = self.model.expect("called with a model installed");
        let here = self.eagle_raw(boot);
        m.ref_boot = boot;
        m.ref_true = here;
        m
    }

    fn eagle_raw(&self, boot: i64) -> i64 {
        self.snapshot().eagle_at(boot).unwrap_or(boot)
    }

    /// The mapping alone — model plus any slew in flight — small and `Copy`, for publishing to readers that must not take a lock (the audio callbacks).
    pub fn snapshot(&self) -> Snapshot {
        Snapshot { model: self.model.map(|m| (m.ref_boot, m.ref_true, m.rate_ppb)), slew: self.slew.map(|s| (s.from.ref_boot, s.from.ref_true, s.from.rate_ppb, s.since)) }
    }

    /// True time at a boot instant, with its uncertainty and lock (spec §4.4 `eagle_of`). Before any fit the boot clock itself is returned, marked `Free`.
    pub fn eagle_of(&self, boot: i64) -> Stamp {
        let Some(m) = self.model else {
            return Stamp { eagle: boot, uncertainty_ns: u32::MAX, source: LockSource::Free };
        };
        let eagle = self.eagle_raw(boot);
        // A slew still in flight is error we know we carry: count the remaining gap in the uncertainty.
        let gap_ns = osc_to_ns(m.eagle_at(boot) - eagle);
        let unc = m.unc_ns_at(boot).saturating_add(gap_ns); // WHY/PROOF: holdover growth is unbounded in principle (a device in a drawer for years); a saturated u64 then clamps to u32::MAX below, which reads as "no idea", the honest answer
        Stamp { eagle, uncertainty_ns: u32::try_from(unc).unwrap_or(u32::MAX), source: m.source_at(boot) }
    }

    /// The boot instant at which true time reads `eagle` (spec §4.4 `mono_of`), for scheduling playout.
    pub fn boot_of(&self, eagle: i64) -> i64 {
        match self.model {
            Some(m) => m.boot_at(eagle),
            None => eagle,
        }
    }

    pub fn model(&self) -> Option<Model> {
        self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: i64 = OSC_PER_SEC;

    /// A synthetic reference: true = boot + offset0 + boot_since_start·rate, measured with symmetric delay `d` plus an asymmetric queue on some exchanges.
    fn exchanges(offset0: i64, rate_ppb: i64, start: i64, n: usize, every: i64, d: i64) -> Vec<Exchange> {
        (0..n)
            .map(|i| {
                let boot = start + i as i64 * every;
                let offset = offset0 + ((boot - start) as i128 * rate_ppb as i128 / 1_000_000_000) as i64;
                Exchange { boot, offset, delay: d }
            })
            .collect()
    }

    /// Offset and rate come back from a clean window: +80 ppm (the spec's test crystal) over an hour, to within a ppb and an oscillation-scale offset.
    #[test]
    fn recovers_offset_and_rate() {
        let ex = exchanges(1_870 * S / 1000, 80_000, 1_000 * S, 40, 90 * S, S / 100);
        let (m, r) = fit(&ex, LockSource::Ntp).unwrap();
        assert!((m.rate_ppb - 80_000).abs() <= 1, "rate {}", m.rate_ppb);
        let last = ex.last().unwrap();
        assert!((m.eagle_at(last.boot) - (last.boot + last.offset)).abs() <= 2, "offset at the newest exchange");
        assert_eq!(r.window, 40);
        let bins = ex.iter().map(|e| e.boot.div_euclid(BIN_OSC)).collect::<std::collections::BTreeSet<_>>().len();
        assert_eq!(r.kept, bins, "each 5-minute bin's best exchange (≤4 per bin, the quartile rounds to one)");
    }

    /// Queued (high-delay) exchanges carry the asymmetric-path error; the quartile filter leaves them out.
    #[test]
    fn high_delay_exchanges_are_filtered() {
        let mut ex = exchanges(0, 0, 0, 16, 120 * S, S / 200);
        for e in ex.iter_mut().step_by(2) {
            e.delay = S / 4;
            e.offset += S / 10; // 100 ms of one-way queueing
        }
        let (m, _) = fit(&ex, LockSource::Ntp).unwrap();
        assert_eq!(m.eagle_at(0), 0, "only the clean exchanges were fitted");
    }

    /// A looser reading never outranks a tighter one in the same bin: the ±1 s exchange is filtered, not averaged in.
    #[test]
    fn a_looser_exchange_in_the_same_burst_is_ignored() {
        let tight = Exchange { boot: 10 * S, offset: S / 2, delay: S / 500 };
        let loose = Exchange { boot: 11 * S, offset: -5 * S, delay: 2 * S };
        let (m, _) = fit(&[tight, loose], LockSource::Ntp).unwrap();
        assert_eq!(m.eagle_at(10 * S), 10 * S + S / 2);
        assert!(m.unc_ns.abs_diff(1_000_000) <= 1, "half the tight exchange's 2 ms delay, got {}", m.unc_ns);
    }

    /// One burst of queries fits an offset but no rate: the rate is unknown, the uncertainty grows at the spec's 2 ppm.
    #[test]
    fn a_single_burst_fits_no_rate() {
        let ex = exchanges(5 * S, 0, 0, 8, S / 10, S / 50);
        let (m, _) = fit(&ex, LockSource::Ntp).unwrap();
        assert_eq!(m.rate_ppb, 0);
        assert_eq!(m.rate_unc_ppb, DEFAULT_RATE_UNC_PPB);
        let hour_later = m.fit_boot + 3600 * S;
        assert_eq!(m.unc_ns_at(hour_later) - m.unc_ns, 7_200_000, "2 ppm for an hour = 7.2 ms");
    }

    /// Past the holdover age the lock reads Holdover and the stamp degrades as its uncertainty grows.
    #[test]
    fn holdover_marks_and_grows() {
        let mut c = TrueClock::new();
        assert_eq!(c.eagle_of(0).source, LockSource::Free);
        c.feed(&exchanges(0, 0, 0, 8, S / 10, S / 1000), LockSource::Ntp, S, false);
        assert_eq!(c.eagle_of(2 * S).source, LockSource::Ntp);
        let late = c.eagle_of(3 * 3600 * S);
        assert_eq!(late.source, LockSource::Holdover);
        assert!(late.degraded(), "3 h at 2 ppm is ~21.6 ms, past the 5 ms line");
    }

    /// While a wave is live a new fit never steps: a 10 ms correction walks in at 50 µs/s, and output never jumps.
    #[test]
    fn a_live_wave_slews_instead_of_stepping() {
        let mut c = TrueClock::new();
        c.feed(&exchanges(0, 0, 0, 8, S / 10, S / 1000), LockSource::Ntp, S, false);
        let before = c.eagle_of(2 * S).eagle;
        c.feed(&exchanges(S / 100, 0, 2 * S, 64, S / 100, S / 1000), LockSource::Ntp, 2 * S, true);
        assert!((c.eagle_of(2 * S).eagle - before).abs() <= 1, "no step at the refit");
        let one_s = c.eagle_of(3 * S).eagle - (3 * S);
        assert!((one_s - (50_000 * S / 1_000_000_000)).abs() <= 1, "50 µs slewed after one second, got {one_s}");
        let done = c.eagle_of(2 * S + 300 * S);
        assert_eq!(done.eagle, 2 * S + 300 * S + S / 100, "the full 10 ms is in after 200 s");
    }

    /// Scheduling inverts the map to within one oscillation.
    #[test]
    fn boot_of_inverts_eagle_of() {
        let ex = exchanges(123_456_789, -45_000, 0, 20, 300 * S, S / 100);
        let (m, _) = fit(&ex, LockSource::Ntp).unwrap();
        for b in [0, 1_000 * S, 5_000 * S, 90_000 * S] {
            assert!((m.boot_at(m.eagle_at(b)) - b).abs() <= 1, "boot {b}");
        }
    }
}
