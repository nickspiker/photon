---
name: feedback_render_never_touches_vault
description: HARD (field 2026-09-18 ANR): the UI/render thread never makes a vault call — presence answers come from a cache a worker fills; anything needing a vault read goes thru queue_job; the ANR trace was main parked in kete::FlatStorage's mpsc recv behind 1.4 s chunk-commit fsyncs
metadata:
  type: feedback
---

**Conviction (Nick's phone log 2026-09-18 02:58, v0.99.0):** `ExitInfo: ANR … Input dispatching timed out … main thread Native: syscall ← Thread::park ← mpmc recv ← kete::FlatStorage` — the UI thread was inside a vault round trip while `kete: SLOW commit — 1 op(s) … in 1460ms (every vault caller blocked behind it)` flushed incoming wave chunks. `blob_present` on a chunked blob walked all 545 chunk addresses thru `read_stored` (545 round trips) on the render thread, re-walked after every chunk store's `blob_presence_forget`: "render took 2514 / 6546 / 7784 / 10103 ms on Conversation". Nick's note: "opening conversations sometimes takes quite a while … definitely should be offline and appear in the conversation with thumbnails generated, rather than preloading first."

**Rule:** the render / event thread reads caches only. `storage::blob_present(h)` = known-true from the cache; `blob_present_known(h)` = Some/None (None queues the hash in `PRESENCE_WANTED`); `blob_present_or_pending(h)` for labels/layout (optimistic); `blob_present_probe_now(h)` = the vault work, ONLY on a worker (the tick's `drain_presence_probes` → `queue_job`, the chunk receive job, the rare row-landing fetch gate). Any new vault read wanted by the render goes the same way: cache + wanted list + worker + repaint edge.

**How to apply:** grep for `device_vault()` / `read_stored` / `blob_manifest` / `blob_chunks_held` / `blob_load` reachable from render.rs, photon_app.rs display helpers, or an event handler — each is a stall waiting for a slow commit. Related: [[project_android_hang_nag]] (vault open on a worker, the same class), [[project_vault_op_latency]].
