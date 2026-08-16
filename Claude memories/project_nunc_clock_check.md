---
name: project_nunc_clock_check
description: "nunc-time is a clock VALIDATOR not photon's clock source; warn-only banner, desktop-only, jump-gated re-check"
metadata: 
  node_type: memory
  type: project
  originSessionId: e5e2b805-7924-4748-8b72-0c8e09057adf
---

nunc-time (sibling crate at ../nunc; package `nunc-time`, **lib crate name `nunc`** so import is `use nunc::`) gives trustworthy wall-clock via multi-source network consensus. In photon it is used as a sanity *validator* of the system clock — NEVER as a timestamp source for data.
ONE load-bearing exception (decided 2026-07-12): the update stamp-window check uses nunc consensus as its `now` at the accept/defer decision — see [[project_update_flow]]. Still never a data stamp, still never disciplines the clock.
VERIFIED ON DEVICE 2026-07-12: Pixel 8 Pro reached consensus over its own network — "Clock: nunc consensus offset = 0s (±0s, 36/42 sources)" in the pulled VSF log.

**Why:** the braid, message ordering, and avatar newer-wins all stamp eagle-time via `vsf::eagle_time_oscillations()` (local clock). They need a monotonic, unique-per-704ps-tick local stamp; nunc's ~seconds latency + ±confidence interval would break them. nunc is the right tool for time-as-security-window decisions, not per-event data stamps.

**How it works (decided with user 2026-06-29):**
- Warn-only: a consensus offset > 30s raises an amber "clock off — Nm behind/ahead" banner (same style as "storage degraded"); the clock is NEVER silently corrected. Open source ⇒ rely on honesty, surface anomalies loudly.
- One-shot at startup (a few seconds after attest, off-thread), PLUS a mid-session re-check gated on a `ClockJumpDetector` (monotonic `Instant` vs wall `SystemTime`; >1h unexplained skew triggers a fresh nunc query). The jump is the cheap TRIGGER; nunc is the arbiter. Banner tracks the LATEST verdict (not sticky).
- **All platforms except Redox** (un-gated 2026-07-12, user insisted: updates require nunc on Android since most users are Android). The old "ring needs the NDK" desktop-only gating was UNTESTED CAUTION — ring 0.17 cross-compiles + links fine for aarch64-linux-android under android-env.sh (which already carried ring's clang symlinks); note nunc's `https` feature also rides rustls's ring backend, not just roughtime. Redox is the one real gap (ring has no Redox target → same-signature Unavailable stub; eventual fix = pure-Rust rustls CryptoProvider in nunc, ferros-era). Dep lives in the `cfg(not(target_os="redox"))` block (merged with thread-priority's); Android call sites pass wake=None (Choreographer redraws, drain runs every tick).

Lives in [src/network/clock_check.rs]; wired into PhotonApp (`clock_off: Option<i64>`, `clock_jump`, `drain_clock_check`, banner in the Ready render). Sign convention: offset = consensus − system (positive = system BEHIND).

Pre-existing, unrelated: Android `cargo check` fails on blake3 needing `aarch64-linux-android-clang` (NDK not on PATH) — fails on clean main too; real Android builds go thru the NDK-configured path. Also `network::pt::tests::test_concurrent_transfers_same_peer` fails on clean main (stream-id counter not deterministic).
