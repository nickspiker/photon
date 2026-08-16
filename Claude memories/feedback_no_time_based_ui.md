---
name: feedback_no_time_based_ui
description: never time-based UI — no auto-expiring toasts/banners/transitions; state changes are event-driven and dismissals are interaction-cleared (click or keystroke)
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 255ce95c-54a6-4041-960d-ad3a2007e55b
---

No UI behaviour may be driven by a timer: no N-second toasts, no auto-dismissing banners, no delayed transitions.
Confirmations and hints are event-SHOWN and interaction-CLEARED — they sit until the user's next click or keystroke acknowledges them (photon: the `clear_hints` path).

**Why:** stated bluntly after I shipped a 4s "Device added" toast ("should NOT be doing any time based anything"); the codebase already carried the doctrine on hints ("event-driven — never hover or time"). A timed dismissal can vanish before the user looks, and timers add frames/wakeups for nothing.

**How to apply:** when adding any transient message, wire its clearing into the existing interaction paths (photon: `clear_hints`, called on every left-press and keystroke), never into `tick` elapsed-time checks. Animation phases (spectrum wave, hourglass wobble) are fine — they're rendering, not state.

**Extends to networking (2026-07-03): all PUSH-based, no poll cadences.** The only permitted timers are ones an external protocol/transport forces: the pairing slot's 5-min freshness re-post, reconnect/degraded-transport backoff, NAT keepalive pings. Everything else rides hub push events (`pair_evt` frames: matched/fleet/fstate) — see the join loop and `spawn_fleet_event_sub` for the pattern.

Related: [[feedback_orb_settings_panel]].
