---
name: messaging-solidity-phase-a
description: "Friend-plane laws from Phase A (SHIPPED 2026-08-08) + B4: gaps are transport, advance-on-send, per-lane heads, garbage-decrypt the sole fork evidence; B1-B3 since shipped"
metadata:
  type: project
---

Root of the week-long "clutch completes then messages stick" (fixed 4701adc, 2026-08-08): the receiver's salt update passed a party id to a lane-label-keyed `set_last_plaintext`, silently no-opping — message 2 on a fresh lane garbage-decrypted, the streak re-keyed, repeat.

Phase A gave the friend plane the fleet plane's CRDT laws (single writer per lane, idempotent ingest, heads compared on every edge, repair is convergence):
- A1 (6809681): gaps are transport, not fork evidence; strand-miss HOLDS instead of skip-and-fork; chat/ACK path authenticated.
- A2 (3cbf70a): advance on SEND, not on ACK — the lane pipelines (in-flight window 4); ACK is a pure receipt.
- A3 (7a56b04): per-lane heads in SyncRecord + a record for EVERY conversation, so a lost frame always recovers via anti-entropy.
- A4 (020e4d4): garbage-decrypt is the SOLE fork evidence; converge (era supersede via forced head exchange) before any re-key.
- B4 (2026-08-09): HealLatch, per-key fleet.locked/released (locks commute), canonical roster merge tie-break, roster-pull backstop on the refold edge.

**Why:** the friend plane wedged as serial lockstep over a lossy transport with ~14 patchwork recoveries ending in destructive re-keys; the fleet plane converged because it is a CRDT.
**How to apply:** never reintroduce gap-streak re-keys or ACK-gated advance. B1→B3 is shipped ([[fleet-epoch-arc-design]]); later lane design: docs/lanes.md, docs/braid.md; [[lane-rotation-wedge-heal]].
