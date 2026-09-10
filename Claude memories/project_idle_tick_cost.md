---
name: project_idle_tick_cost
description: "Idle-screen CPU on Android CUT 2026-09-10 (100% → ~10% of a core): per-vsync tick profile (`PERF: tick profile`, `FLUOR: frames`), the repeated syscalls/KDFs found, idle frame cadence at 30 Hz"
metadata: 
  node_type: memory
  type: project
  originSessionId: 87fd33b1-f610-46f9-b464-4fe0dd1e3280
  modified: 2026-09-10T21:25:39.830Z
---

Measured with the phone on adb (2026-09-10): an idle Ready screen ran the whole protocol tick at every vsync (117–119 Hz) at 5–7 ms a tick plus a 1.3 ms window lock/post that drew nothing = a full core. Instruments now in the tree: `PERF: tick profile — … ms per tick: <section> …` every 30 s (sections = `timed!` labels + `proto:` regions of advance_protocol + `status:` regions of check_status_updates), `PERF: redraw storm` under >30 dirty ticks/s, and fluor's `FLUOR: frames — frames/s, dirty/s; tick/render/present ms`.
What the profile convicted, all fixed: `udp::get_local_ip()` (a socket bind+connect per call from six per-tick paths → cached 1 s); `identity_party_id` (an ed25519 derivation per contact per tick via `our_party_id` → memoized per seed in `identity_pid_cache`); the stalled-address peer-store harvest (cloned the whole store per tick → once a second); `spawn_next_pending_keygen` (recomputed ceremony owners per tick → 4 Hz); fluor Android present (lock/post when every buffer already held the frame → skipped after 3 clean hits); Kotlin frame callback (idle → 24 ms cadence after 3 unchanged frames, any touch/key wakes vsync). Result: 30 frames/s idle, ~3 ms a tick, ~10% of a core. Still ~3 ms a tick: CLUTCH drains ~1.3 ms, status drain ~1 ms, settle_self_display ~0.5 ms.
This is the concrete form of the old [[project_fluor_busy_loop]] complaint on Android (desktop's wake_at spin is a separate, still-open path).

**Why:** Nick: "we really gotta work on this UI latency"; a hot idle phone is battery and heat, and the tick sat in front of every input.
**How to apply:** before optimizing UI, read the newest `tick profile` line in the phone log; anything that runs per tick must be O(contacts) with no syscalls, KDFs, clones of stores, or vault reads. Related: [[project_android_hang_nag]], [[project_render_storm_lag]].
