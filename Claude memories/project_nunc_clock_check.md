---
name: project_nunc_clock_check
description: "nunc consensus IS photon's time base (time_base, BOOTTIME anchor); a server clock refusal re-anchors only when tight; true_clock weights exchanges by (min_delay/delay)²"
metadata:
  type: project
---

nunc-time (../nunc, package `nunc-time`, lib name `nunc`) = multi-source network clock consensus.
- **Time base (since v99, 2026-09-17):** `time_base` anchors nunc's consensus on CLOCK_BOOTTIME (std Instant stops in sleep — photon fell behind by every minute asleep); without a consensus `now_osc()` is the system clock.
- Warn-only clock banner (offset > 30 s, amber; the system clock is never silently corrected); one-shot after attest plus a re-check on a `ClockJumpDetector` edge. src/network/clock_check.rs; sign: offset = consensus − system.
- The update stamp-window check uses nunc as its `now` ([[project_update_flow]]).
- **Server refusal:** the worker's window refusal carries `server_now`; `time_base::adopt_from_server(server_now, rtt)` re-anchors and retries once.
- **2026-10-10 CONVICTED (slow uplink, Nick's phone):** a 6.5 MB log submit crawled for minutes, the server read the stale stamp as "107 s behind", and adopt_from_server hard-RESET the clock over a ±13 ms consensus → messages sorted after their replies, express answers dropped as stale, silent waves. FIX: a verdict whose uncertainty dwarfs the held lock (20×) is fed as one loose exchange, never a reset (time_base.rs); `log_put` gets a 15-minute window (fgtw-bootstrap).
- **Same day (MacBook):** that loose exchange was then fit together with nunc's consensus unweighted → RATE read −3.6 % → 4000+ wave re-aligns, playout −42 s. FIX: true_clock.rs weights every exchange by (min_delay / delay)², and a rate needs a span of exchanges that carry weight. The loose-exchange gate is safe only because of this weighting.

**Why:** every ordering, stale-guard and the wave aligner sit on this clock.
**How to apply:** a server's clock verdict from a long request measures the request, not the clock — never reset on it. The epoch never moves ([[feedback_epoch_fixed]]).
