---
name: edges-not-timers
description: "Nick's design doctrine — react on event edges, never on timers/debounces/polls"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: db6a8a12-db99-4e93-ae59-33a1753e3d5e
  modified: 2026-07-31T15:13:22.004Z
---

Nick has rejected timer-based designs twice on 2026-07-31 alone: the 1s zoom-persist debounce ("Yeah, no timings. 1s is bad. Should be upon release.") and earlier the whole polling architecture ("EVERYTHING should be push/doorbell featured, not timing based").

**Why:** a timer is a guess about when the real event happened; the edge IS the event. Debounces add latency and mask the actual trigger; polls burn battery/data and fake boots in logs.

**How to apply:** when persisting or reacting to state changes, find the release/settle/completion EDGE (key-up, onScaleEnd, ACK arrival, hub push) and hook that. If a platform layer doesn't surface the edge (like Kotlin's onScaleEnd being a no-op), wire it through rather than approximating with a timer. Timers are acceptable only as fallback heartbeats for edges that can genuinely be lost, and should be rare + slow. See also [[connection-flow-revision]].

**2026-08-16 reinforcement (window geometry):** the "settle edge" (save when two consecutive TICK samples match) was a debounce riding the tick clock — a timer in edge costume — and in the field it never fired at all (idle ticks run on the presence cadence, so the "settle" waited on time that never came). Nick caught it from the WORD alone: "settle? did you put a timer in again???". The tell: if the behavior changes with tick rate, it is a timer no matter what it is named. The fix was the real events (fluor didn't even forward WindowEvent::Moved — add the hook, don't poll around its absence) + flush on lifecycle edges (focus-lost, close).
