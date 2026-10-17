---
name: project_fluor_busy_loop
description: "OPEN since 2026-09-08: photon main thread can pin 100% CPU — PhotonApp::wake_at returns bare Instant::now() while animating, and fluor WaitUntil(past deadline) spins"
metadata:
  type: project
---

Symptom (leviathan, 2026-09-08): photon main thread ~100% CPU, workers idle, log silent; intermittent.
Mechanism: fluor host/app.rs `about_to_wait` does `ControlFlow::WaitUntil(app.wake_at())`; a past deadline wakes instantly → render → same past deadline → silent busy loop.
`PhotonApp::wake_at` (src/ui/photon_app/driver.rs) still returns bare `Instant::now()` while `animating` (Attesting | Searching | `add_in_flight` | wave Ringing | watched Active wave field); a stuck flag = permanent full-rate loop.
Prime suspect `add_in_flight`: audit every add/search failure/timeout/offline path for a missed clear.
Latent: the presence arm returns `now` when `last_presence_ping` is None, gated on Ready only, while its servicing path is also gated on `app_watching()` (Android = foreground) → the guards disagree → spin.
Android's idle-tick cost was a separate path, closed ([[project_idle_tick_cost]]).

**Why:** a hot main thread costs battery, heat and input latency, and logs nothing.
**How to apply:** floor animation wakes to now + one frame; guarantee add_in_flight/Searching clear on failure and timeout; make each wake arm's guard identical to its servicing path's; confirm with a rate-limited log of (state, add_in_flight, winning arm) inside wake_at.
