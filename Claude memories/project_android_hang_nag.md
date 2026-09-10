---
name: project_android_hang_nag
description: "CLOSED 2026-09-10 — Android \"not responding\" at launch = 5 s vault open on the UI thread (mirror live-walk at every open); manifestus converged fast path; remaining 1.5 s = resume walk hashing every furrow"
metadata: 
  node_type: memory
  type: project
  originSessionId: 87fd33b1-f610-46f9-b464-4fe0dd1e3280
  modified: 2026-09-10T21:25:08.410Z
---

Nick's Pixel 8 showed the ANR prompt at nearly every launch (2026-09-10). Bugreport symbolization (llvm-symbolizer against the archived unstripped .so) put the main thread in `kete open_device_shared → manifestus verified_replicate → hamt walk_child → blake3` for ~5 s: every open compared both mirrors' live set block by block (72 MB a side), inside `PERF: resume load — vault 5339ms … (UI thread)`.
Fixed in manifestus bd1e12a: same generation + same slot + identical head block + equal extents ⇒ skip the walk. Launch vault phase is now ~1.5 s, all of it `Vault::open → resume → walk_live_collect`, which reads and hashes every furrow block of every extent (grows with kept waves) — the next cut if launch needs to be faster.
Second cause, v0.89.19–.22 only: arming the native fault handler under libsigchain made faults non-fatal (see [[project_crash_handler_sigchain]]).

**Why:** the ANR threshold is 5 s of unanswered input; the vault phase crossed it as the vault grew with recordings.
**How to apply:** read `PERF: resume load` in any phone log first; a `vault` number near 5000 ms is this class. Vault-size-proportional work never belongs on the UI thread. Related: [[project_render_storm_lag]], [[project_vault_op_latency]].
