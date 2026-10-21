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

**2026-10-21 CONVICTED + FIXED (Nick's MacBook: ~112 wakes/s idle on Conversation all night):** two `wake_at` arms outlived the 2026-09-02 demand-driven presence change — the fleet re-fold arm still fired on Ready/Conversation while advance_protocol only re-folds on a watched Settings page, so `last_fleet_refold` never advanced and its deadline sat in the past forever → fluor's 16 ms floor = permanent ~60 Hz (macOS waker doubles it). Fix: protocol.rs gates (`watched_presence_surface`, `presence_due_at`, `lan_beacon_due_at`, `fleet_refold_due_at`) read by BOTH the pump and wake_at; draft_deadline None while drafts can't persist; dead presence_ping_interval / last_interaction removed. fluor: `wake_at() == None` now sets ControlFlow::Wait (winit's WaitUntil is sticky — a passed instant spins). Plus fluor `on_window_shown()` (ShowWindow/Raise/Dock reopen) → photon set_window_visible(true). Still open: a stuck animation flag still floors at 60 Hz.
