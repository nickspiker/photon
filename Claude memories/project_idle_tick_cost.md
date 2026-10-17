---
name: project_idle_tick_cost
description: "Idle-screen CPU on Android CUT 2026-09-10 (a full core → ~10%): tick profile instruments, per-tick syscalls/KDFs/clones removed, idle frames at 30 Hz"
metadata:
  type: project
---

2026-09-10: an idle Ready screen ran the whole protocol tick every vsync (~118 Hz, 5–7 ms each) = a full core.
Fixed: `udp::get_local_ip()` cached 1 s (was a socket bind per call from six per-tick paths); `identity_party_id` memoized (`identity_pid_cache`); peer-store harvest once a second; `spawn_next_pending_keygen` at 4 Hz; fluor Android skips present when every buffer already holds the frame; the Kotlin frame callback idles at 24 ms after 3 unchanged frames.
Result: ~3 ms a tick, 30 frames/s idle, ~10% of a core.
Instruments: `PERF: tick profile` (every 30 s, per `timed!` / `proto:` / `status:` region), `PERF: redraw storm`, fluor `FLUOR: frames`.

**Why:** Nick: "we really gotta work on this UI latency"; a hot idle phone is battery and heat, and the tick sits in front of every input.
**How to apply:** before optimizing UI, read the newest `tick profile` line; per-tick work must be O(contacts) with no syscalls, KDFs, store clones or vault reads ([[feedback_render_never_touches_vault]]). Desktop's wake_at spin is separate: [[project_fluor_busy_loop]].
