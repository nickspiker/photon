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

REOPENED AND CLOSED AGAIN 2026-09-12: back at 5.1 s (vault 3.7 s) as the vault grew with kept waves; the ANR record reads "Input dispatching timed out — waited 5001 ms for FocusEvent" with main in Native inside kete::FlatStore → mpsc recv, i.e. the launch, not a stall a person feels (Nick: "it never actually stalls"). Fix: `open_session_vault` runs on a `vault-open` worker; `poll_resume_vault` (tick) calls `finish_resume_load` (the extracted 320-line back half: contacts, messages, chains, keypairs, settings, then Ready + query_resume + pings) when the handle lands. Rule stands: vault-size-proportional work never on the UI thread; the resume walk's furrow hashing is still the cost underneath.

CLOSED AT THE ROOT 2026-09-12 (manifestus c37b511): the open walked every furrow and extent block of every value to take its seal hash — a full read of the vault, growing with kept waves. Now `walk_tree_collect` reads the index only and marks furrows live by position (`LiveSet::referenced`, hashless; `is_live` answers by position for those, the plow reads each block's seal itself). KAT: 23 reads to open a vault of 1,025 live data blocks. Nick: "treed open was the initial design; zero reason we verify the whole vault." Next, per Nick: conversations addressed most-recent-first, old content trimmed as the ring rotates past a size/date limit.
