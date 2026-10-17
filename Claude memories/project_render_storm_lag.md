---
name: project-render-storm-lag
description: "CLOSED 2026-08-16: UI lag = a per-tick avatar probe read queued on the vault mutex behind persist writers; fixed by probe caches + the kete librarian thread"
metadata:
  type: project
---

Field 2026-08-15: clicks up to 1 s, beachballs. Region timers (`status pre-loop` / `status deferred`) convicted `check_status_updates` at median 492 ms / max 1794 ms: the avatar sweep's per-tick `has_cached_avatar_from_seed` took kete's vault mutex, which persist workers held thru whole encrypted table writes.
Fixed: probe results cached per party id (the UI touches the vault zero times in steady state); kete 931fd90 THE LIBRARIAN — one thread owns the manifestus engine, ops ride an mpsc mailbox, reads are answered before each mutation (a read waits ≤ one write). Field-confirmed on v0.54.7: p50 64 ms, zero pre-loop stalls.
Side lesson: a stale sibling vsf checkout hid an already-fixed bug — fast-forward path deps together ([[macbook-trails-remote]]).

**Why:** every background write had become a UI stall.
**How to apply:** this is the origin of [[feedback_render_never_touches_vault]]; when lag appears, add region timers to name the stall before guessing.
