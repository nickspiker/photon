//! Photon's own clock (Nick 2026-09-03): the device's wall clock is a DISPLAY PREFERENCE that belongs to the human — set it five minutes fast if you like — and photon has no business correcting it. What photon needs is a comparable time base, which is a different thing entirely.
//!
//! **Why this exists.** Message order is `(timestamp, blake3(content))` and `timestamp` is the SENDING device's clock, so two devices whose clocks disagree mis-interleave a conversation permanently — trust in the friend is irrelevant, since two honest clocks that disagree still produce a wrong order. Measured on Nick's own fleet: the phone ran **1.87 s ahead** of the desktop, steady, with automatic time ON. That is not a misconfiguration anyone can fix: Android took the time from NITZ (carrier, whole-second granularity) and its time detector only steps the clock when a suggestion differs by more than `mSystemClockUpdateThresholdMillis = 2000`, so a sub-2-second error is deliberately left alone. Sub-second agreement is simply not obtainable from an OS clock by configuration, on any settings screen.
//!
//! **The shape.** nunc measures the offset out of band and reports it anchored to the local instant it was measured against (`offset_et` + `local_et`). We store that against a MONOTONIC anchor, never against the wall clock: an offset stored relative to the system clock is invalidated the moment the human nudges that clock, whereas a monotonic anchor is immune — set the system clock to next Tuesday and ordering does not flinch.
//!
//! Two hard rules: photon NEVER writes the system clock, and no stored row is ever restamped (timestamps are row identity, and the anti-entropy digest is order-dependent — restamping would re-walk history across the whole fleet forever).

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
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

/// The disciplined clock (docs/lock.md §4): exchanges against the reference, fitted to offset + rate over the suspend-counting boot clock. Replaces the single anchor, which carried an offset but no rate — every hour of drift since the last consensus was error it could not see.
static CLOCK: Mutex<crate::network::true_clock::TrueClock> = Mutex::new(crate::network::true_clock::TrueClock::new());

/// The clock's mapping for readers that must not block — the audio callbacks stamp every captured and rendered frame, and a real-time thread may not wait on [`CLOCK`]'s mutex. A seqlock: the writer (always under CLOCK, so writers never race) makes the sequence odd, stores, makes it even; a reader retries until it saw the same even sequence on both sides of its loads.
static SNAP_SEQ: AtomicU64 = AtomicU64::new(0);
/// [has model, ref_boot, ref_true, rate_ppb, has slew, from_boot, from_true, from_rate, since].
static SNAP: [AtomicI64; 9] = [const { AtomicI64::new(0) }; 9];

fn publish(clock: &crate::network::true_clock::TrueClock) {
    let s = clock.snapshot();
    let m = s.model.unwrap_or_default();
    let w = s.slew.unwrap_or_default();
    let vals = [s.model.is_some() as i64, m.0, m.1, m.2, s.slew.is_some() as i64, w.0, w.1, w.2, w.3];
    SNAP_SEQ.fetch_add(1, Ordering::AcqRel);
    for (slot, v) in SNAP.iter().zip(vals) {
        slot.store(v, Ordering::Relaxed);
    }
    SNAP_SEQ.fetch_add(1, Ordering::AcqRel);
}

/// True time at a boot instant WITHOUT a lock — for the audio callbacks (spec §5.1: every captured sample stamped by the HAL's clock thru TrueClock). Before any fit, the wall clock carried to that instant, like [`stamp_at`].
pub fn eagle_at_boot_rt(boot: i64) -> i64 {
    let snap = loop {
        let s1 = SNAP_SEQ.load(Ordering::Acquire);
        if s1 & 1 == 1 {
            std::hint::spin_loop();
            continue;
        }
        let v: [i64; 9] = std::array::from_fn(|i| SNAP[i].load(Ordering::Relaxed));
        std::sync::atomic::fence(Ordering::Acquire);
        if SNAP_SEQ.load(Ordering::Relaxed) == s1 {
            break crate::network::true_clock::Snapshot { model: (v[0] != 0).then_some((v[1], v[2], v[3])), slew: (v[4] != 0).then_some((v[5], v[6], v[7], v[8])) };
        }
    };
    snap.eagle_at(boot).unwrap_or_else(|| vsf::eagle_time_oscillations() + (boot - boot_osc()))
}

/// Nanoseconds on the boot clock as oscillations — the unit every boot instant here is kept in.
pub fn boot_ns_to_osc(ns: i64) -> i64 {
    (ns as i128 * crate::OSC_PER_SEC as i128 / 1_000_000_000) as i64
}

/// The newest oscillation count [`stamp_osc`] has handed out. A refresh that corrects us BACKWARD must never let the next stamp land behind one already in a row — that would invert our own conversation against itself.
static LAST_ISSUED: AtomicI64 = AtomicI64::new(i64::MIN);

/// The boot clock, public for the clock-check worker, which converts each observation's local instant onto it at the moment the query returns.
pub fn boot_now() -> i64 {
    boot_osc()
}

/// Feed reference exchanges (the clock-check worker's, already on the boot clock) and refit. `no_step` = a wave is live, so the new fit slews in at 50 µs/s instead of jumping (spec §4.3). Every fit is logged (spec §4.3).
pub fn feed(exchanges: &[crate::network::true_clock::Exchange], no_step: bool) {
    let now = boot_osc();
    let report = {
        let mut c = CLOCK.lock().unwrap();
        let r = c.feed(exchanges, crate::network::true_clock::LockSource::Ntp, now, no_step);
        publish(&c);
        r
    };
    if let Some(r) = report {
        let st = now_stamp();
        crate::logf!(
            "Clock: fit — window = {}, kept = {}, min delay = {} µs, residual = {} µs, rate = {} ppb, uncertainty = {} µs, lock = {}{}",
            r.window,
            r.kept,
            r.min_delay_ns / 1000,
            r.residual_ns / 1000,
            r.rate_ppb,
            st.uncertainty_ns / 1000,
            st.source.name(),
            if no_step { " (slewing: a wave is live)" } else { "" }
        );
    }
}

/// Adopt a nunc CONSENSUS when it brought no precise exchange of its own (HTTPS-only draws): one exchange at its anchor, whose delay is twice the consensus width so the fit reports that width as its uncertainty. `offset_osc` = true − system wall clock, `local_osc` = the wall clock reading it is anchored to, straight from `NuncTime`.
pub fn adopt(offset_osc: i64, confidence_osc: i64, local_osc: i64) {
    let (boot, wall) = (boot_osc(), vsf::eagle_time_oscillations());
    // The consensus was true at `local_osc` by the WALL clock; carry that instant onto the boot clock by how long ago it was.
    let at = boot - (wall - local_osc);
    feed(&[crate::network::true_clock::Exchange { boot: at, offset: local_osc + offset_osc - at, delay: 2 * confidence_osc }], false);
}

/// Adopt the SERVER'S verdict (2026-09-17, Theresa's phone: "Timestamp outside valid window" on every log submit, and nothing to say whether photon's clock was ahead or behind): FGTW refused a frame we stamped and answered with its own clock in the refusal. That server is NTP-disciplined and is the one judging the window, so its reading outranks the whole fit — the window RESTARTS from it and the clock steps (a clock proven wrong by over a minute is not slewed thru). The server read its clock somewhere inside the trip, so the midpoint is the estimate and half the trip the honest width, floored at a quarter second. The next consensus refines it thru the ordinary fit.
pub fn adopt_from_server(server_now_osc: i64, rtt_osc: i64) {
    let rtt = rtt_osc.max(0); // WHY/PROOF: the RTT is measured across a wall clock that can step backwards mid-flight — a negative round trip is 0
    let delay = rtt.max(crate::OSC_PER_SEC / 2);
    let boot = boot_osc();
    let before = now_osc();
    let true_now = server_now_osc + rtt / 2;
    {
        let mut c = CLOCK.lock().unwrap();
        c.reset_to(crate::network::true_clock::Exchange { boot, offset: true_now - boot, delay }, crate::network::true_clock::LockSource::Ntp);
        publish(&c);
    }
    crate::logf!(
        "Clock: FGTW refused our stamp — re-anchored on the server's clock: photon time moves {} ms, now {} ms off the system clock (±{} ms); a nunc consensus will refine it",
        (true_now - before) * 1000 / crate::OSC_PER_SEC,
        (true_now - vsf::eagle_time_oscillations()) * 1000 / crate::OSC_PER_SEC,
        delay / 2 * 1000 / crate::OSC_PER_SEC
    );
}

/// The server's clock out of a window refusal: the `server_now=<oscillations>` the worker puts in its detail. `None` for any other error.
pub fn server_now_from_detail(detail: &str) -> Option<i64> {
    let at = detail.find("server_now=")? + "server_now=".len();
    let digits: String = detail[at..].chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// True time now, with its uncertainty and lock (spec §4.4 `now`). Before any fit this is the raw system clock marked `Free` — a device that has never been online still has to send, and its stamps say so.
pub fn now_stamp() -> crate::network::true_clock::Stamp {
    stamp_at(boot_osc())
}

/// True time at a past (or future) boot instant — capture and playout stamps (spec §4.4 `eagle_of`).
pub fn stamp_at(boot: i64) -> crate::network::true_clock::Stamp {
    let st = CLOCK.lock().unwrap().eagle_of(boot);
    if st.source == crate::network::true_clock::LockSource::Free {
        // WHY/PROOF: the boot clock's reading is not a date — never disciplined, the wall clock (whatever the OS says) is the only absolute we have, and `Free` tells every reader not to trust it.
        return crate::network::true_clock::Stamp { eagle: vsf::eagle_time_oscillations() + (boot - boot_osc()), ..st };
    }
    st
}

/// The boot instant at which true time will read `eagle` — for scheduling against true time (spec §4.4 `mono_of`).
pub fn boot_of(eagle: i64) -> i64 {
    CLOCK.lock().unwrap().boot_of(eagle)
}

/// True time in oscillations (see [`now_stamp`] for its uncertainty and lock).
pub fn now_osc() -> i64 {
    now_stamp().eagle
}

/// How far past our known time an INSIDER-signed stamp may run and still be believed (2026-09-25, Nick: "future dated peer records need to floor with known local time").
/// Insiders (siblings, molecule members, a friend's era genesis) mint from the nunc-corrected clock, so honest skew is small; but a device whose nunc anchor has not landed yet mints from its raw OS clock, and phones have been seen 30-53 min off.
/// 5 minutes keeps a briefly-unanchored honest device from being refused while still denying any stamp a lasting lever: it can win at most five minutes of newest-wins, never immortality.
/// Stranger records (the open peer store) are not refused at all — they are FLOORED to our clock on arrival (peer_store::floor_to_now).
pub const FUTURE_SKEW_OSC: i64 = 5 * 60 * crate::OSC_PER_SEC;

/// An insider-signed stamp claiming a moment past our known time plus [`FUTURE_SKEW_OSC`]. Refused wherever such a stamp orders, expires or wins something.
/// Every stamp this guards sits inside a SIGNATURE or names a chain era, so it cannot be floored in place without breaking what we re-gossip — the record is refused whole instead.
pub fn from_the_future(stamp: i64) -> bool {
    stamp > now_osc().saturating_add(FUTURE_SKEW_OSC) // WHY/PROOF: now + 5 min cannot overflow for any stamp this century, but `now_osc` falls back to the raw system clock, which is whatever the OS says
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

/// The standing correction for display: `(offset_osc, uncertainty_osc)`, where offset is true − system clock RIGHT NOW (recomputed, so it stays honest if the human moves the system clock after the measurement). `None` until the first fit.
pub fn offset_now() -> Option<(i64, i64)> {
    let st = now_stamp();
    if st.source == crate::network::true_clock::LockSource::Free {
        return None;
    }
    let unc_osc = (st.uncertainty_ns as i128 * crate::OSC_PER_SEC as i128 / 1_000_000_000) as i64;
    Some((st.eagle - vsf::eagle_time_oscillations(), unc_osc))
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

    /// CLOCK and LAST_ISSUED are process globals and the harness runs tests on parallel threads — each test holds the gate and starts from a clean slate (the flake: floor_from_storage's ±1ms adopt landing mid-flight displaced a_worse_measurement's anchor).
    static GATE: Mutex<()> = Mutex::new(());
    fn hold_clean() -> std::sync::MutexGuard<'static, ()> {
        let g = GATE.lock().unwrap_or_else(|p| p.into_inner());
        {
            let mut c = CLOCK.lock().unwrap();
            *c = crate::network::true_clock::TrueClock::new();
            publish(&c);
        }
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
        assert!(offset_now().is_some_and(|(_, c)| (c - crate::OSC_PER_SEC / 4).abs() <= crate::OSC_PER_SEC / 1000), "the quarter-second floor");
        // A nunc consensus at ±3 ms then outranks the server's width.
        adopt(0, crate::OSC_PER_SEC * 3 / 1000, vsf::eagle_time_oscillations());
        assert!(offset_now().is_some_and(|(_, c)| (c - crate::OSC_PER_SEC * 3 / 1000).abs() <= crate::OSC_PER_SEC / 10_000), "the ±3 ms consensus outranks it");
        // The refusal detail parses; anything else does not.
        assert_eq!(server_now_from_detail("Timestamp outside valid window: client is 1821s behind the server (server_now=123456789)"), Some(123456789));
        assert_eq!(server_now_from_detail("bad_signature: nope"), None);
    }

    /// The lock-free reader the audio callbacks use agrees with the locked clock, before and after a fit.
    #[test]
    fn the_real_time_reader_matches_the_locked_clock() {
        let _g = hold_clean();
        let close = |a: i64, b: i64| (a - b).abs() < crate::OSC_PER_SEC / 1000;
        assert!(close(eagle_at_boot_rt(boot_osc()), now_osc()), "unfitted: both carry the wall clock");
        adopt(crate::OSC_PER_SEC * 7, crate::OSC_PER_SEC / 1000, vsf::eagle_time_oscillations());
        assert!(close(eagle_at_boot_rt(boot_osc()), now_osc()), "fitted: the published snapshot is the model");
        assert!(close(eagle_at_boot_rt(boot_osc()) - vsf::eagle_time_oscillations(), crate::OSC_PER_SEC * 7));
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
