---
name: project-manifestus-tombstone-bug
description: "CLOSED 2026-07-24: manifestus delete left the committed pointer, so slot reuse turned the tombstone into a seal failure on the next fresh open; fixed manifestus 56bde9a + vaultfix"
metadata:
  type: project
---

`Hamt::delete` zeroed the leaf but left the index pointer; one plow lap later an append reused the lba and the pointer flipped to a seal mismatch, killing every FRESH open at the resume `walk_live` (in-session RAM kept working, so it surfaced at the auto-update re-exec).
Fix @ manifestus 56bde9a: delete COW-unlinks the pointer; resume prunes stale pointers; `Vault::open_repairing` + the `vaultfix` salvage bin; tests/tombstone.rs reproduces the signature.

**Why:** a corruption that appears only on a fresh open points at stale on-disk references, not RAM state.
**How to apply:** `vaultfix` is the idempotent salvage tool; keep forensic images before repairing.
