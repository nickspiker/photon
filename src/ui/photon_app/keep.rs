//! WHAT THIS DEVICE KEEPS (Nick 2026-11-17: "would be nice to have the sliders for audio, video, pigeons and photons adjustable... I really don't need to keep record of every single frame of a beam I made from 6 months ago").
//!
//! Each kind of held bytes (a wave's recording, a beam's video, a pigeon's file) has two limits, an age and a total size, and whichever trims more wins (docs/retention.md §2).
//! Past either limit the oldest let their BYTES go on this device: the photon, its preview and its waveform stay, and a starred (★ kept) one never goes.
//! Blob bytes never enter key material (a row's pointer is what the braid weaves), so trimming them leaves every braid intact.
//! The limits are this device's own, or the fleet's when it follows the fleet (the Beam page's switch); either way this device trims to them without asking a sibling (Nick 2026-11-17: "if it is, then whatever that limit is, gone!").
//! A trimmed hash joins `space.evicted` (space.rs), so no automatic fetch brings it back; a deliberate fetch (Play, Save) still does.
//! Photons themselves are not trimmed yet: every row feeds the digest siblings converge on, so a row pruned on one device would be served back to it (the floor design is open).
//! Ages count from the row's own time; the receipt record docs/retention.md designs is not built.

use super::*;
use crate::types::AttachRole;

/// The kinds a limit covers, in the sliders' order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KeepKind {
    Wave,
    Beam,
    Pigeon,
}

impl KeepKind {
    pub(super) const ALL: [KeepKind; 3] = [KeepKind::Wave, KeepKind::Beam, KeepKind::Pigeon];

    /// The kind a row's held bytes belong to; `None` for a row with no file, and for a wave's envelope, which IS the card's face.
    pub(super) fn of(m: &crate::types::ChatMessage) -> Option<Self> {
        match m.file.as_ref()?.role {
            AttachRole::WaveAudio => Some(Self::Wave),
            AttachRole::BeamVideo => Some(Self::Beam),
            AttachRole::File => Some(Self::Pigeon),
            AttachRole::WaveEnv => None,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// The setting keys, an age and a size per kind, in the sliders' order: an age in oscillations, a size in bytes, absent or `u0(false)` for no limit.
pub(super) const KEEP_KEYS: [&str; 6] = ["keep.wave.age", "keep.wave.size", "keep.beam.age", "keep.beam.size", "keep.pigeon.age", "keep.pigeon.size"];

/// One day in oscillations, the age slider's left end.
const DAY_OSC: u64 = 86_400 * crate::OSC_PER_SEC as u64;
/// The age slider spans this many doublings above a day (to about eleven years); its far right end is no limit.
const AGE_DOUBLINGS: f64 = 12.;
/// The size slider starts at 2^26 bytes (64 MiB) and spans this many doublings (to 1 TiB); its far right end is no limit.
const SIZE_FLOOR_DOUBLINGS: f64 = 26.;
const SIZE_DOUBLINGS: f64 = 14.;

/// The limits this device trims to: an age (oscillations) and a total size (bytes) per kind, `None` = no limit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct KeepLimits {
    pub(super) age: [Option<u64>; 3],
    pub(super) size: [Option<u64>; 3],
}

impl KeepLimits {
    fn any(&self) -> bool {
        self.age.iter().chain(self.size.iter()).any(Option::is_some)
    }
}

/// One limit from its slider's position: continuous in doublings (the handle never steps), the right end no limit.
pub(super) fn limit_from_slider(age: bool, v: f32) -> Option<u64> {
    if v >= 1. {
        return None;
    }
    let v = v as f64;
    Some(if age { (DAY_OSC as f64 * (v * AGE_DOUBLINGS).exp2()) as u64 } else { (v * SIZE_DOUBLINGS + SIZE_FLOOR_DOUBLINGS).exp2() as u64 })
}

/// A slider's position for a stored limit (the inverse of [`limit_from_slider`]).
pub(super) fn slider_from_limit(age: bool, limit: Option<u64>) -> f32 {
    match limit {
        None => 1.,
        Some(n) if age => ((n as f64 / DAY_OSC as f64).log2() / AGE_DOUBLINGS) as f32,
        Some(n) => (((n as f64).log2() - SIZE_FLOOR_DOUBLINGS) / SIZE_DOUBLINGS) as f32,
    }
}

/// An age limit as the page reads it: the doubling count where the base speaks doublings, else whole days.
pub(super) fn age_label(osc: u64) -> String {
    if crate::dms_ui() {
        crate::dms_age(osc as i64)
    } else {
        tr(Msg::KeepDays(&crate::fmt_num64(osc / DAY_OSC))).into_owned()
    }
}

/// Is a row of this age past the limit? A stamp ahead of our clock is no age at all.
fn past(age: i64, limit: u64) -> bool {
    age > 0 && age as u64 > limit
}

/// A held row the trim weighs: its kind, its time, its bytes' hash and size.
pub(super) type KeepRow = (KeepKind, i64, [u8; 32], u64);

/// The rows to let go, given which are held here: per kind, newest first, a row goes once it is older than the age limit or the kind's running total passes the size limit.
pub(super) fn select(mut rows: Vec<KeepRow>, limits: KeepLimits, now: i64, held: impl Fn(&[u8; 32]) -> bool) -> Vec<KeepRow> {
    rows.sort_unstable_by(|a, b| b.1.cmp(&a.1));
    let mut total = [0u64; 3];
    let mut out = Vec::new();
    for r in rows {
        if !held(&r.2) {
            continue;
        }
        let k = r.0.index();
        // WHY/PROOF: a row's size can be a peer's claim off the wire (any u64): a wrapped total would fall back under the limit and keep what it should let go.
        total[k] = total[k].saturating_add(r.3);
        // WHY/PROOF: a row's time can be a peer's stamp (any i64), so the difference must not wrap into a small age.
        let age = now.saturating_sub(r.1);
        let too_old = limits.age[k].is_some_and(|a| past(age, a));
        let too_big = limits.size[k].is_some_and(|s| total[k] > s);
        if too_old || too_big {
            out.push(r);
        }
    }
    out
}

impl PhotonApp {
    /// Does this device follow the fleet's limits? Only by choice, as on the Beam page.
    pub(super) fn keep_follows_fleet(&self) -> bool {
        self.fleet_settings.as_ref().is_some_and(|fs| fs.device_local(KEEP_KEYS[0]).is_some() && fs.linked(KEEP_KEYS[0]))
    }

    fn keep_value(&self, key: &str) -> Option<vsf::VsfType> {
        let fs = self.fleet_settings.as_ref()?;
        if self.keep_follows_fleet() { fs.effective(key) } else { fs.device_local(key) }.cloned()
    }

    /// The limits from the settings into `keep_limits` and the sliders (a slider under the finger keeps its place); a change trims.
    pub(super) fn load_keep_settings(&mut self) {
        let mut limits = KeepLimits::default();
        for (i, key) in KEEP_KEYS.iter().enumerate() {
            let v = self.keep_value(key).filter(|v| !matches!(v, vsf::VsfType::u0(_))).and_then(|v| v.as_u64());
            if i % 2 == 0 {
                limits.age[i / 2] = v;
            } else {
                limits.size[i / 2] = v;
            }
        }
        let drag = self.keep_drag;
        for (i, sl) in self.keep_sliders.iter_mut().enumerate() {
            if drag != Some(i) {
                let limit = if i % 2 == 0 { limits.age[i / 2] } else { limits.size[i / 2] };
                sl.set_value(slider_from_limit(i % 2 == 0, limit));
            }
        }
        if limits != self.keep_limits {
            self.keep_limits = limits;
            self.keep_trim("the limits changed");
        }
    }

    /// A released slider writes its limit where this device keeps it (its own map, unless it follows the fleet).
    pub(super) fn commit_keep_slider(&mut self, i: usize) {
        let v = self.keep_sliders[i].value();
        let value = match limit_from_slider(i % 2 == 0, v) {
            Some(n) => vsf::VsfType::u6(n),
            None => vsf::VsfType::u0(false),
        };
        if !self.ensure_fleet_settings() {
            return;
        }
        let follows = self.keep_follows_fleet();
        let key = KEEP_KEYS[i];
        let now = vsf::eagle_time_oscillations();
        let fs = self.fleet_settings.as_mut().unwrap();
        if !follows && fs.linked(key) {
            fs.set_link(key, false, now);
        }
        if fs.set(key, value.clone(), now) {
            crate::logf!("SETTINGS: {} = {:?} ({})", key, value, if follows { "fleet" } else { "device-local" });
            self.persist_and_push_settings();
        }
        self.load_keep_settings();
    }

    /// Link every limit to the fleet, or keep them on this device (each keeps its position as it changes over).
    pub(super) fn set_keep_follow(&mut self, follow: bool) {
        if !self.ensure_fleet_settings() {
            return;
        }
        let now = vsf::eagle_time_oscillations();
        let fs = self.fleet_settings.as_mut().unwrap();
        let mut changed = false;
        for key in KEEP_KEYS {
            // A key never written yet gets its own entry first, so "this device chose" is on record either way.
            if fs.device_local(key).is_none() {
                changed |= fs.set_link(key, false, now);
            }
            changed |= fs.set_link(key, follow, now);
        }
        if changed {
            crate::logf!("SETTINGS: keep.* {}", if follow { "follow the fleet" } else { "this device only" });
            self.persist_and_push_settings();
        }
        self.load_keep_settings();
    }

    /// Would this device hold the row's bytes at all? False past its kind's age limit, and for bytes trimmed or evicted here, so automatic fetches skip them.
    pub(super) fn keep_wants(&self, m: &crate::types::ChatMessage) -> bool {
        let Some(f) = m.file.as_ref() else { return true };
        if self.space_evicted.contains(&f.addr()) {
            return false;
        }
        let Some(k) = KeepKind::of(m) else { return true };
        // WHY/PROOF: as in `select`, a row's time may be a peer's stamp (any i64).
        let age = vsf::eagle_time_oscillations().saturating_sub(m.timestamp);
        m.star_osc > 0 || self.keep_limits.age[k.index()].is_none_or(|a| !past(age, a))
    }

    /// The trim, on an edge (the session up, a limit changed, a recording or file landed or kept): the candidates go to a worker, which reads what is held here and lets go of what is past a limit.
    pub(super) fn keep_trim(&mut self, why: &str) {
        if !self.keep_limits.any() || self.session.is_none() {
            return;
        }
        if self.keep_rx.is_some() {
            self.keep_again = true;
            return;
        }
        let rows: Vec<KeepRow> = self
            .conversations
            .iter()
            .flat_map(|c| c.messages.iter())
            .filter(|m| !m.deleted && m.star_osc <= 0)
            .filter_map(|m| Some((KeepKind::of(m)?, m.timestamp, m.file.as_ref()?.addr(), m.file.as_ref()?.size)))
            .filter(|r| !self.space_evicted.contains(&r.2))
            .collect();
        if rows.is_empty() {
            return;
        }
        crate::logf!("KEEP: weighing {} held rows against the limits ({})", rows.len(), why);
        let limits = self.keep_limits;
        let now = vsf::eagle_time_oscillations();
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = self.event_proxy.clone();
        crate::log_err!(std::thread::Builder::new().name("keep".into()).spawn(move || {
            let out = select(rows, limits, now, crate::storage::blob_present_probe_now);
            for r in &out {
                crate::storage::blob_delete(&r.2);
            }
            crate::log_err!(tx.send(out), "hand the trim to the UI");
            if let Some(w) = wake {
                wake_ui(&w);
            }
        }), "spawn the keep worker");
        self.keep_rx = Some(rx);
    }

    /// A finished trim, once per tick: its hashes join the evicted list. True when something was let go.
    pub(super) fn drain_keep(&mut self) -> bool {
        let landed = match self.keep_rx.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(out)) => Some(out),
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => Some(Vec::new()),
            _ => None,
        };
        let Some(out) = landed else { return false };
        self.keep_rx = None;
        let freed = !out.is_empty();
        if freed {
            let mut count = [0u64; 3];
            let mut bytes = [0u64; 3];
            for (k, _, h, size) in &out {
                count[k.index()] += 1;
                // WHY/PROOF: a peer-claimed size (any u64) summed for the log line; a wrap would misreport, never act.
                bytes[k.index()] = bytes[k.index()].saturating_add(*size);
                self.space_evicted.insert(*h);
            }
            crate::logf!(
                "KEEP: let go of {} wave recordings ({} bytes), {} beam videos ({} bytes), {} pigeons ({} bytes) on this device",
                count[0], bytes[0], count[1], bytes[1], count[2], bytes[2]
            );
            self.persist_space_evicted();
        }
        if std::mem::take(&mut self.keep_again) {
            self.keep_trim("again, an edge came in during the last trim");
        }
        freed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slider_and_its_limit_round_trip_and_the_right_end_is_no_limit() {
        for age in [true, false] {
            assert_eq!(limit_from_slider(age, 1.), None);
            assert_eq!(slider_from_limit(age, None), 1.);
            for v in [0., 0.25, 0.5, 0.75] {
                let back = slider_from_limit(age, limit_from_slider(age, v));
                assert!((back - v).abs() < 1. / 1024., "{age} {v} {back}");
            }
        }
        assert_eq!(limit_from_slider(true, 0.), Some(DAY_OSC));
        assert_eq!(limit_from_slider(false, 0.), Some(1 << 26));
    }

    #[test]
    fn the_age_limit_takes_the_old_and_the_size_limit_keeps_the_newest_that_fit() {
        let day = DAY_OSC as i64;
        let now = 100 * day;
        let h = |n: u8| [n; 32];
        let rows = vec![
            (KeepKind::Beam, now - 90 * day, h(1), 10),
            (KeepKind::Beam, now - 2 * day, h(2), 10),
            (KeepKind::Pigeon, now - day, h(3), 40),
            (KeepKind::Pigeon, now - 3 * day, h(4), 40),
            (KeepKind::Pigeon, now - 5 * day, h(5), 40),
            (KeepKind::Wave, now - 90 * day, h(6), 10),
        ];
        let mut limits = KeepLimits::default();
        limits.age[KeepKind::Beam.index()] = Some(30 * DAY_OSC);
        limits.size[KeepKind::Pigeon.index()] = Some(100);
        let gone: Vec<[u8; 32]> = select(rows.clone(), limits, now, |_| true).into_iter().map(|r| r.2).collect();
        assert_eq!(gone, vec![h(5), h(1)]);
        // Bytes not held here neither count toward a size nor get let go.
        let gone: Vec<[u8; 32]> = select(rows, limits, now, |x| *x != h(3)).into_iter().map(|r| r.2).collect();
        assert_eq!(gone, vec![h(1)]);
        // A stamp from the future is no age; a stamp from the far past cannot wrap into a young one.
        assert!(!past(-5, 0));
        assert!(past(i64::MAX, 1));
    }
}
