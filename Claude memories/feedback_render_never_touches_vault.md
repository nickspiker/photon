---
name: feedback_render_never_touches_vault
description: "HARD (field 2026-09-18 ANR): the UI/render thread never makes a vault call — presence answers come from a cache a worker fills; anything needing a vault read goes thru queue_job"
metadata:
  type: feedback
---

**Conviction (Nick's phone, 2026-09-18, v0.99.0):** ANR with main parked in `kete::FlatStorage`'s mpsc recv behind a 1.46 s chunk-commit fsync; `blob_present` on a chunked blob walked 545 chunk addresses thru `read_stored` on the render thread, re-walked after every chunk store → Conversation renders of 2.5-10 s.

**Rule:** the render / event thread reads caches only.
- `storage::blob_present(h)` = known-true from the cache; `blob_present_known(h)` = Some/None (None queues the hash in `PRESENCE_WANTED`); `blob_present_or_pending(h)` for labels/layout.
- `blob_present_probe_now(h)` = the vault work, ONLY on a worker (`drain_presence_probes` → `queue_job`, the chunk receive job, the row-landing fetch gate).
- Any new vault read the render wants: cache + wanted list + worker + repaint edge.

**Why:** every vault call can block behind a slow commit; on the UI thread that is an ANR.
**How to apply:** grep for `device_vault()` / `read_stored` / `blob_manifest` / `blob_chunks_held` / `blob_load` reachable from render.rs, display helpers or event handlers — each is a stall. Related: [[project_android_hang_nag]], [[project_vault_op_latency]].
