//! Photon's own clock (Nick 2026-09-03): the device's wall clock is a DISPLAY PREFERENCE that belongs to the human — set it five minutes fast if you like — and photon has no business correcting it. What photon needs is a comparable time base, which is a different thing entirely.
//!
//! **Why this exists.** Message order is `(timestamp, blake3(content))` and `timestamp` is the SENDING device's clock, so two devices whose clocks disagree mis-interleave a conversation permanently — trust in the friend is irrelevant, since two honest clocks that disagree still produce a wrong order. Measured on Nick's own fleet: the phone ran **1.87 s ahead** of the desktop, steady, with automatic time ON. That is not a misconfiguration anyone can fix: Android took the time from NITZ (carrier, whole-second granularity) and its time detector only steps the clock when a suggestion differs by more than `mSystemClockUpdateThresholdMillis = 2000`, so a sub-2-second error is deliberately left alone. Sub-second agreement is simply not obtainable from an OS clock by configuration, on any settings screen.
//!
//! **The shape.** nunc measures the offset out of band and reports it anchored to the local instant it was measured against (`offset_et` + `local_et`). We store that against a MONOTONIC anchor, never against the wall clock: an offset stored relative to the system clock is invalidated the moment the human nudges that clock, whereas a monotonic anchor is immune — set the system clock to next Tuesday and ordering does not flinch.
//!
//! Two hard rules: photon NEVER writes the system clock, and no stored row is ever restamped (timestamps are row identity, and the anti-entropy digest is order-dependent — restamping would re-walk history across the whole fleet forever).

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;

/// THE CLOCK THAT COUNTS THROUGH SLEEP (field 2026-09-17: Nick's phone slept and photon's time fell 30 minutes behind — "Timestamp outside valid window: diff=1821s", growing to 3175 s an hour later — because `std::time::Instant` is CLOCK_MONOTONIC on Linux/Android, which STOPS while the device is suspended; every minute the phone slept was a minute the anchor never saw). CLOCK_BOOTTIME is the monotonic clock that includes suspend; macOS has mach_continuous_time for the same; Windows' GetTickCount64 counts through sleep at millisecond grain. All immune to the wall clock, which is the property the anchor exists for.
fn boot_osc() -> i64 {
    // Nanoseconds → oscillations in i128 (LOCK: no float on anything that names an instant).
    let ns_to_osc = |ns: i128| -> i64 { (ns * crate::OSC_PER_SEC as i128 / 1_000_000_000) as i64 };
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) } == 0 {
            return ns_to_osc(ts.tv_sec as i128 * 1_000_000_000 + ts.tv_nsec as i128);
        }
    }
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn mach_continuous_time() -> u64;
            fn mach_timebase_info(info: *mut [u32; 2]) -> i32;
        }
        let mut tb = [0u32; 2];
        if unsafe { mach_timebase_info(&mut tb) } == 0 && tb[1] != 0 {
            let ns = unsafe { mach_continuous_time() } as i128 * tb[0] as i128 / tb[1] as i128;
            return ns_to_osc(ns);
        }
    }
    #[cfg(target_os = "windows")]
    {
        extern "system" {
            fn GetTickCount64() -> u64;
        }
        let ms = unsafe { GetTickCount64() };
        return ns_to_osc(ms as i128 * 1_000_000);
    }
    #[allow(unreachable_code)]
    {
        // Fallback (redox, an exotic host): the process-monotonic clock — correct while awake, blind to suspend.
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        let start = START.get_or_init(std::time::Instant::now);
        ns_to_osc(start.elapsed().as_nanos() as i128)
    }
}

/// One measurement, anchored where it can't be invalidated: `true_osc` was true at the moment `boot` read (the suspend-counting monotonic clock, in oscillations), and that clock is the only thing we extrapolate from.
struct Anchor {
    boot: i64,
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

    let now_boot = boot_osc();
    let mut slot = ANCHOR.lock().unwrap();
    let replace = match slot.as_ref() {
        None => true,
        Some(cur) => {
            let age = now_boot - cur.boot;
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
        boot: now_boot,
        true_osc: local_osc + offset_osc + elapsed_since_local,
        confidence_osc,
    });
    crate::logf!(
        "Clock: anchor set — offset {} ms (±{} ms); photon time is now independent of the system clock",
        offset_osc * 1000 / crate::OSC_PER_SEC,
        confidence_osc * 1000 / crate::OSC_PER_SEC
    );
}

/// Adopt the SERVER'S verdict (2026-09-17, Theresa's phone: "Timestamp outside valid window" on every log submit, and nothing to say whether photon's clock was ahead or behind): FGTW refused a frame we stamped and answered with its own clock in the refusal. That server is NTP-disciplined and is the one judging the window, so its reading outranks whatever anchor we hold — this REPLACES the standing anchor unconditionally (the standing one just proved itself wrong by over a minute), at the width the round trip allows: the server read its clock somewhere inside the trip, so the midpoint is the estimate and half the trip the honest width, floored at a quarter second. A later nunc consensus (±ms) refines it thru [`adopt`]'s ordinary rule.
pub fn adopt_from_server(server_now_osc: i64, rtt_osc: i64) {
    let rtt = rtt_osc.max(0); // WHY/PROOF: the RTT is measured across a wall clock that can step backwards mid-flight — a negative round trip is 0
    let confidence_osc = (rtt / 2).max(crate::OSC_PER_SEC / 4);
    let true_now = server_now_osc + rtt / 2;
    let system_now = vsf::eagle_time_oscillations();
    let before = now_osc();
    *ANCHOR.lock().unwrap() = Some(Anchor { boot: boot_osc(), true_osc: true_now, confidence_osc });
    crate::logf!(
        "Clock: FGTW refused our stamp — re-anchored on the server's clock: photon time moves {} ms, now {} ms off the system clock (±{} ms); a nunc consensus will refine it",
        (true_now - before) * 1000 / crate::OSC_PER_SEC,
        (true_now - system_now) * 1000 / crate::OSC_PER_SEC,
        confidence_osc * 1000 / crate::OSC_PER_SEC
    );
}

/// The server's clock out of a window refusal: the `server_now=<oscillations>` the worker puts in its detail. `None` for any other error.
pub fn server_now_from_detail(detail: &str) -> Option<i64> {
    let at = detail.find("server_now=")? + "server_now=".len();
    let digits: String = detail[at..].chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// True time in oscillations, extrapolated from the anchor on the monotonic clock. Falls back to the raw system clock when nunc has never reached consensus — a device that has never been online still has to send.
pub fn now_osc() -> i64 {
    let slot = ANCHOR.lock().unwrap();
    match slot.as_ref() {
        Some(a) => a.true_osc + (boot_osc() - a.boot),
        None => vsf::eagle_time_oscillations(),
    }
}

/// Raise the stamp floor to at least `osc` — called at vault load for every OUTGOING row, because [`LAST_ISSUED`] is runtime-only and starts over each boot. Without this, a nunc correction on an ahead-running clock (the phone's steady +1.87 s) pulls the FIRST post-restart stamp behind rows sent minutes earlier in the previous session — and rows never restamp, so the inversion is permanent (field 2026-09-07: Nick's out-of-order messages). The floor only ever rises; inbound rows are excluded on purpose (their stamps are the sender's clock — clamping ours to theirs would let one fast friend clock drag our whole timeline forward).
pub fn raise_floor(osc: i64) {
    let mut cur = LAST_ISSUED.load(Ordering::Relaxed);
    while osc > cur {
        match LAST_ISSUED.compare_exchange_weak(cur, osc, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return,
            Err(seen) => cur = seen,
        }
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
    let true_now = a.true_osc + (boot_osc() - a.boot);
    Some((true_now - vsf::eagle_time_oscillations(), a.confidence_osc))
}

#[cfg(test)]
mod tests {
    /// ONE EPOCH (LOCK stage 1, 2026-09-24): nunc keeps its own small integer Eagle module (it carries no vsf dependency), so this pins the two together — the same epoch constant and the same round-half-up arithmetic, to the oscillation, across the instants the fleet actually stamps.
    #[test]
    fn nunc_and_vsf_agree_to_the_oscillation() {
        assert_eq!(nunc::eagle::EAGLE_EPOCH_UNIX_SECS, vsf::types::EAGLE_EPOCH_UNIX_SECS);
        assert_eq!(nunc::eagle::OPS as u64, vsf::OSCILLATIONS_PER_SECOND);
        for (s, n) in [(0i64, 0u32), (1_790_000_000, 1), (1_790_000_000, 999_999_999), (-14_182_940, 500_000_000), (-100, 3)] {
            assert_eq!(nunc::eagle::from_unix(s, n), vsf::types::from_unix_ns(s, n), "({s}, {n})");
        }
    }

    use super::*;

    /// ANCHOR and LAST_ISSUED are process globals and the harness runs tests on parallel threads — each test holds the gate and starts from a clean slate (the flake: floor_from_storage's ±1ms adopt landing mid-flight displaced a_worse_measurement's anchor).
    static GATE: Mutex<()> = Mutex::new(());
    fn hold_clean() -> std::sync::MutexGuard<'static, ()> {
        let g = GATE.lock().unwrap_or_else(|p| p.into_inner());
        *ANCHOR.lock().unwrap() = None;
        LAST_ISSUED.store(i64::MIN, Ordering::Relaxed);
        g
    }

    #[test]
    fn a_server_refusal_re_anchors_over_a_standing_anchor_and_nunc_refines_it() {
        let _g = hold_clean();
        // A tight but WRONG standing anchor: photon time ten minutes ahead of true.
        adopt(crate::OSC_PER_SEC * 600, crate::OSC_PER_SEC / 1000, vsf::eagle_time_oscillations());
        let system = vsf::eagle_time_oscillations();
        assert!(now_osc() - system > crate::OSC_PER_SEC * 599);
        // The server says its clock is the system clock (true), over a 400 ms round trip: the anchor is replaced despite being "tighter", at ±250 ms (the quarter-second floor beats 200 ms).
        adopt_from_server(system, crate::OSC_PER_SEC * 2 / 5);
        let off = now_osc() - vsf::eagle_time_oscillations();
        assert!(off.abs() < crate::OSC_PER_SEC, "re-anchored near true time, got {} ms", off * 1000 / crate::OSC_PER_SEC);
        assert_eq!(offset_now().map(|(_, c)| c), Some(crate::OSC_PER_SEC / 4));
        // A nunc consensus at ±3 ms then outranks the server's width.
        adopt(0, crate::OSC_PER_SEC * 3 / 1000, vsf::eagle_time_oscillations());
        assert_eq!(offset_now().map(|(_, c)| c), Some(crate::OSC_PER_SEC * 3 / 1000));
        // The refusal detail parses; anything else does not.
        assert_eq!(server_now_from_detail("Timestamp outside valid window: client is 1821s behind the server (server_now=123456789)"), Some(123456789));
        assert_eq!(server_now_from_detail("bad_signature: nope"), None);
    }

    /// Stamps never repeat and never regress, even when the anchor is corrected backward mid-stream — the row-identity guarantee.
    #[test]
    fn stamps_are_strictly_increasing_across_a_backward_correction() {
        let _g = hold_clean();
        adopt(crate::OSC_PER_SEC * 10, crate::OSC_PER_SEC / 100, vsf::eagle_time_oscillations());
        let a = stamp_osc();
        let b = stamp_osc();
        assert!(b > a, "two stamps in a row must differ and advance");
        // A correction that yanks us ten seconds backward, tighter so it is adopted.
        adopt(-crate::OSC_PER_SEC * 10, crate::OSC_PER_SEC / 1000, vsf::eagle_time_oscillations());
        let c = stamp_osc();
        assert!(c > b, "a backward correction must not re-issue a stamp behind one already used");
    }

    /// The restart hole: a floor raised from stored rows keeps a post-restart corrected stamp from landing behind them.
    #[test]
    fn floor_from_storage_prevents_post_restart_inversion() {
        let _g = hold_clean();
        let high = vsf::eagle_time_oscillations() + crate::OSC_PER_SEC * 30;
        raise_floor(high);
        // A correction pulling "now" well behind the stored row (the ahead-clock case after restart).
        adopt(-crate::OSC_PER_SEC * 60, crate::OSC_PER_SEC / 1000, vsf::eagle_time_oscillations());
        let s = stamp_osc();
        assert!(s > high, "a corrected stamp must never land behind a stored outgoing row");
    }

    /// A looser reading never displaces a tighter fresh one; the anchor keeps the best measurement it has.
    #[test]
    fn a_worse_measurement_does_not_replace_a_better_fresh_one() {
        let _g = hold_clean();
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
