---
name: project-manifestus-tombstone-bug
description: "manifestus fast-delete tombstone bug: delete left the committed pointer; plow-lap reuse = seal-fail on next fresh open; FIXED @ manifestus 56bde9a 2026-07-24, desktop vault repaired zero-loss; PUBLISH PENDING to all devices"
metadata: 
  node_type: memory
  type: project
  originSessionId: bf3c2e39-d57b-4469-8848-1780b1b5c927
---

The 2026-07-24 desktop "storage degraded / seal verification failed" vault corruption root cause: Hamt::delete zeroed the leaf but left the committed index pointer in place; safe only while the slot stays zero — one plow lap later an ordinary append reused the lba and the tombstone pointer flipped to a seal mismatch, killing every FRESH open at resume walk_live (in-session RAM state kept working, so it surfaced only at the auto-update re-exec).
NOT the reap (its delta ordering is sound), NOT a crash, NOT the dual-engine mode. Both mirrors identical because the engine wrote it.

FIX @ manifestus 56bde9a: delete COW-unlinks the pointer (prune) after the zero pass; resume prunes pre-existing stale pointers (heals not-yet-reused tombstones in every live vault on next open+commit); Vault::open_repairing + vaultfix bin (idempotent salvage CLI); inspect decodes extent leaves + furrows (the old "8 blocks failed / reachable vs live gap" was an inspect gap); tests/tombstone.rs reproduces the exact signature.
Desktop vault REPAIRED with vaultfix (gen 1685, all spec checks pass, ZERO user values lost — the only pruned referent was an already-deleted key's tombstone). Forensic images kept at ~/vault-forensics-2026-07-24/.

EVERY device still runs the tombstone-vulnerable engine until a build carrying manifestus 56bde9a ships — any delete on any device mints a future landmine that detonates on a later fresh open after slot reuse. Resume-prune heals existing tombstones once devices update. Publish is the priority.

Related: [[project-storage-layering]], [[project-clutch-token-asymmetry]] (the update re-exec that surfaced this).
