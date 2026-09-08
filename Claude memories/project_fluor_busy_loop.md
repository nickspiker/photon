---
name: project_fluor_busy_loop
description: OPEN (handoff 2026-09-08, do AFTER the era-ratchet plan): photon main thread pins 100% CPU — fluor about_to_wait WaitUntil(past deadline) spins; PhotonApp::wake_at arms returning bare Instant::now() (animating: Attesting/Searching/add_in_flight/Ringing) and a presence arm whose guard differs from its servicing path on Android
metadata:
  type: project
---

Handoff from the rustdesk agent (leviathan, 2026-09-08). Symptom: photon-messenger main thread (tid==pid) at ~100% CPU, state R, all workers idle, log not growing (hot path logs nothing); intermittent (0.2% ↔ 100% for minutes).
Mechanism: fluor host/app.rs `about_to_wait` does `ControlFlow::WaitUntil(self.app.wake_at())`; a deadline already in the past wakes instantly → re-render → same past deadline → silent busy loop.
`PhotonApp::wake_at` (src/ui/photon_app/driver.rs ~2349) returns bare `Instant::now()` when `animating` = Launch(Attesting) | Searching | `add_in_flight` | active_call Ringing (driver.rs ~2354-2363). If any flag sticks, permanent full-rate loop. Prime suspect `add_in_flight`: set input.rs ~319, cleared only input.rs ~1020 and launch.rs ~408 — audit every add/search failure/timeout/offline path for a missed clear (log showed several Pending… rows + RELAY drops to offline peers).
Latent Android hazard: presence arm (driver.rs ~2365) returns `now` when `last_presence_ping.is_none()` gated only on AppState::Ready, but the servicing path (protocol.rs ~38-45) is additionally gated on `app_watching()` (desktop hardcoded true; Android = foreground) → guards disagree → spin. Make each wake arm's guard identical to its servicing path's guard.
Fixes to do: floor the animation wake to now+~16ms (same visuals, no hard spin); guarantee add_in_flight/Searching clear on failure+timeout; match guards; confirm with a 1/sec rate-limited log of (state, add_in_flight, winning arm) inside wake_at.
