---
name: edges-not-timers
description: "Nick's design doctrine — react on event edges, never on timers/debounces/polls"
metadata:
  type: feedback
---

Nick rejects timer-based designs: "Yeah, no timings. 1s is bad. Should be upon release." (zoom-persist debounce, 2026-07-31); "EVERYTHING should be push/doorbell featured, not timing based."

**Why:** a timer is a guess about when the real event happened; the edge IS the event. Debounces add latency and mask the trigger; polls burn battery/data.

**How to apply:** hook the release/settle/completion EDGE (key-up, onScaleEnd, ACK arrival, hub push). If a platform layer doesn't surface the edge, wire it thru (e.g. add the missing fluor WindowEvent::Moved forward) rather than approximating with a timer. Timers only as rare, slow fallback heartbeats for edges that can genuinely be lost.
The tell (2026-08-16, window geometry "settle when two TICK samples match" — "settle? did you put a timer in again???"): if behaviour changes with tick rate, it is a timer no matter its name. Flush on lifecycle edges (focus-lost, close) instead. UI side: [[feedback_no_time_based_ui]].

**Every ask gets an answer (2026-10-21, Nick: "I don't like timers. how do we fix the 20s check?"):** a silence timer exists only because some outcome sends nothing. Make every outcome an event instead: data (progress), an explicit negative reply (attach_none "not held"), the transport giving up (PT newly-dead address / relay "recipient offline" → StatusUpdate::PeerUnreachable), or the offline verdict. A push nobody asked for counts as the answer to an implicit ask of its sender (fetch_expect_push), so no "push window". The transport's own retry ladder is the one sanctioned clock underneath.
