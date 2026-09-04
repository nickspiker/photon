//! Photon's own clock (Nick 2026-09-03): the device's wall clock is a DISPLAY PREFERENCE that belongs to the human — set it five minutes fast if you like — and photon has no business correcting it. What photon needs is a comparable time base, which is a different thing entirely.
//!
//! **Why this exists.** Message order is `(timestamp, blake3(content))` and `timestamp` is the SENDING device's clock, so two devices whose clocks disagree mis-interleave a conversation permanently — trust in the friend is irrelevant, since two honest clocks that disagree still produce a wrong order. Measured on Nick's own fleet: the phone ran **1.87 s ahead** of the desktop, steady, with automatic time ON. That is not a misconfiguration anyone can fix: Android took the time from NITZ (carrier, whole-second granularity) and its time detector only steps the clock when a suggestion differs by more than `mSystemClockUpdateThresholdMillis = 2000`, so a sub-2-second error is deliberately left alone. Sub-second agreement is simply not obtainable from an OS clock by configuration, on any settings screen.
//!
//! **The shape.** nunc measures the offset out of band and reports it anchored to the local instant it was measured against (`offset_et` + `local_et`). We store that against a MONOTONIC anchor, never against the wall clock: an offset stored relative to the system clock is invalidated the moment the human nudges that clock, whereas a monotonic anchor is immune — set the system clock to next Tuesday and ordering does not flinch.
//!
//! Two hard rules: photon NEVER writes the system clock, and no stored row is ever restamped (timestamps are row identity, and the anti-entropy digest is order-dependent — restamping would re-walk history across the whole fleet forever).

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// One measurement, anchored where it can't be invalidated: `true_osc` was true at the moment `mono` read, and the monotonic clock is the only thing we extrapolate from.
struct Anchor {
    mono: Instant,
    true_osc: i64,
    confidence_osc: i64,
}

static ANCHOR: Mutex<Option<Anchor>> = Mutex::new(None);

/// The newest oscillation count [`stamp_osc`] has handed out. A refresh that corrects us BACKWARD must never let the next stamp land behind one already in a row — that would invert our own conversation against itself.
static LAST_ISSUED: AtomicI64 = AtomicI64::new(i64::MIN);

/// Adopt a nunc verdict. `offset_osc` = true − local, `local_osc` = the local clock reading it is anchored to, both straight from `NuncTime` (no sampling here — the caller cannot know when the consensus was true, which is the entire reason nunc reports its own anchor).
///
/// A worse measurement never replaces a better fresh one: an unlucky source draw (HTTPS-only, ±500 ms) must not degrade an anchor that a good draw set at ±5 ms. It IS adopted once the standing anchor goes stale, because a wide fresh reading beats a narrow ancient one.
pub fn adopt(offset_osc: i64, confidence_osc: i64, local_osc: i64) {
    /// Past this age the standing anchor has drifted (quartz runs ±20-50 ppm ⇒ ~180 ms per hour), so any fresh reading outranks it.
    const STALE_OSC: i64 = 2 * 3600 * crate::OSC_PER_SEC;

    let now_mono = Instant::now();
    let mut slot = ANCHOR.lock().unwrap();
    let replace = match slot.as_ref() {
        None => true,
        Some(cur) => {
            let age = (now_mono.duration_since(cur.mono).as_secs_f64() * crate::OSC_PER_SEC as f64) as i64;
            confidence_osc <= cur.confidence_osc || age > STALE_OSC
        }
    };
    if !replace {
        crate::logf!(
            "Clock: keeping the standing anchor — new reading is looser (±{} ms vs ±{} ms)",
            confidence_osc * 1000 / crate::OSC_PER_SEC,
            slot.as_ref().map(|c| c.confidence_osc).unwrap_or(0) * 1000 / crate::OSC_PER_SEC
        );
        return;
    }
    // The consensus was true at `local_osc` by the LOCAL clock; that instant has already passed by however long the verdict took to reach us, so carry it forward on the monotonic clock rather than pretending it is now.
    let elapsed_since_local = vsf::eagle_time_oscillations() - local_osc;
    *slot = Some(Anchor {
        mono: now_mono,
        true_osc: local_osc + offset_osc + elapsed_since_local,
        confidence_osc,
    });
    crate::logf!(
        "Clock: anchor set — offset {} ms (±{} ms); photon time is now independent of the system clock",
        offset_osc * 1000 / crate::OSC_PER_SEC,
        confidence_osc * 1000 / crate::OSC_PER_SEC
    );
}

/// True time in oscillations, extrapolated from the anchor on the monotonic clock. Falls back to the raw system clock when nunc has never reached consensus — a device that has never been online still has to send.
pub fn now_osc() -> i64 {
    let slot = ANCHOR.lock().unwrap();
    match slot.as_ref() {
        Some(a) => {
            let elapsed = a.mono.elapsed();
            a.true_osc + (elapsed.as_secs_f64() * crate::OSC_PER_SEC as f64) as i64
        }
        None => vsf::eagle_time_oscillations(),
    }
}

/// [`now_osc`] for anything that becomes a ROW STAMP — additionally guaranteed never to go backward or repeat, so a correction can't re-issue a timestamp behind one already written (row identity is `(timestamp, content)`; a duplicate would read as the same row).
pub fn stamp_osc() -> i64 {
    let mut candidate = now_osc();
    loop {
        let last = LAST_ISSUED.load(Ordering::Relaxed);
        if candidate <= last {
            candidate = last + 1;
        }
        match LAST_ISSUED.compare_exchange_weak(last, candidate, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return candidate,
            Err(_) => continue,
        }
    }
}

/// The standing correction for display: `(offset_osc, confidence_osc)`, where offset is true − system clock RIGHT NOW (recomputed, so it stays honest if the human moves the system clock after the measurement). `None` until the first consensus.
pub fn offset_now() -> Option<(i64, i64)> {
    let slot = ANCHOR.lock().unwrap();
    let a = slot.as_ref()?;
    let elapsed = a.mono.elapsed();
    let true_now = a.true_osc + (elapsed.as_secs_f64() * crate::OSC_PER_SEC as f64) as i64;
    Some((true_now - vsf::eagle_time_oscillations(), a.confidence_osc))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stamps never repeat and never regress, even when the anchor is corrected backward mid-stream — the row-identity guarantee.
    #[test]
    fn stamps_are_strictly_increasing_across_a_backward_correction() {
        adopt(crate::OSC_PER_SEC * 10, crate::OSC_PER_SEC / 100, vsf::eagle_time_oscillations());
        let a = stamp_osc();
        let b = stamp_osc();
        assert!(b > a, "two stamps in a row must differ and advance");
        // A correction that yanks us ten seconds backward, tighter so it is adopted.
        adopt(-crate::OSC_PER_SEC * 10, crate::OSC_PER_SEC / 1000, vsf::eagle_time_oscillations());
        let c = stamp_osc();
        assert!(c > b, "a backward correction must not re-issue a stamp behind one already used");
    }

    /// A looser reading never displaces a tighter fresh one; the anchor keeps the best measurement it has.
    #[test]
    fn a_worse_measurement_does_not_replace_a_better_fresh_one() {
        let local = vsf::eagle_time_oscillations();
        adopt(crate::OSC_PER_SEC / 2, crate::OSC_PER_SEC / 1000, local); // ±1ms
        let (tight, _) = offset_now().expect("anchor set");
        adopt(-crate::OSC_PER_SEC * 5, crate::OSC_PER_SEC, local); // ±1s, wildly different
        let (after, _) = offset_now().expect("anchor still set");
        assert!(
            (after - tight).abs() < crate::OSC_PER_SEC / 100,
            "the ±1s reading must not have displaced the ±1ms one"
        );
    }
}
