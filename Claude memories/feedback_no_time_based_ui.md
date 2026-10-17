---
name: feedback_no_time_based_ui
description: "never time-based UI — no auto-expiring toasts/banners/transitions; state changes are event-driven, dismissals interaction-cleared; networking push-based; fling sheds 1 px/frame"
metadata:
  type: feedback
---

No UI behaviour may be driven by a timer: no N-second toasts, no auto-dismissing banners, no delayed transitions.
Confirmations and hints are event-SHOWN and interaction-CLEARED — they sit until the next click or keystroke (photon: `clear_hints`, called on every left-press and keystroke).

**Why:** after I shipped a 4 s "Device added" toast: "should NOT be doing any time based anything". A timed dismissal can vanish before the user looks, and timers add frames/wakeups for nothing.

**How to apply:**
- Wire any transient message's clearing into the interaction paths, never `tick` elapsed-time checks. Animation phases (spectrum wave, hourglass wobble) are rendering, not state — fine.
- Networking (2026-07-03): all PUSH-based, no poll cadences. Only timers an external protocol forces are allowed (pairing slot 5-min freshness re-post, reconnect backoff, NAT keepalives); everything else rides hub push events.
- Scroll fling law (Nick 2026-10-03): on release keep the LAST frame's pixel delta and shed exactly one pixel per frame to zero (8, 7, 6 … 0), halting redraws at zero; a bound or new finger ends it. No timers, no easing curves (`pane_scroll` + `list_fling` in driver.rs).
Related: [[edges-not-timers]].
