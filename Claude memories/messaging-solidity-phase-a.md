---
name: messaging-solidity-phase-a
description: Phase A (friend-plane messaging solidity) SHIPPED 2026-08-08; Phase B (fleet chain+eggs) is field-gated
metadata: 
  node_type: memory
  type: project
  originSessionId: 296bef77-7c97-45b8-8d90-dc492f93e557
  modified: 2026-08-10T17:05:48.567Z
---

The week-long "clutch completes then messages stick" was ONE root cause: the receiver's salt update passed a party id to a lane-label-keyed `set_last_plaintext`, so it silently no-opped — message 1 on a fresh lane decrypted, message 2 garbage-decrypted, the streak re-keyed, repeat. Fixed in 4701adc.

Phase A then reshaped the friend plane to obey the fleet plane's CRDT-ish laws (the "make both planes the same" ask, 2026-08-08):
- **A1** (6809681): gaps are transport not fork evidence (deleted the ≥8 gap-streak re-key that masked the salt bug all week); strand-miss HOLDS instead of skip-and-forking; chat/ACK path authenticated (known∧not-refused gate, carries sender_pubkey).
- **A2** (3cbf70a): advance-on-SEND not on-ACK — the lane pipelines (in-flight window 4), ACK is a pure receipt; kills the same-key batch and the restart-fork (unpersisted woven_strands stop mattering).
- **A3** (7a56b04): per-lane heads in SyncRecord + a record for EVERY conversation — a lost frame always recovers via anti-entropy; the single max-tip over-reported for multi-device senders.
- **A4** (020e4d4): garbage-decrypt is now the SOLE fork evidence; converge (era-supersede via a forced head exchange) before re-key; owner-only + era-grace guards kept.
- Dead-code cleanup (6dd2f50): removed clear_pending_up_to / process_implicit_ack / get_pending_after.

**Why:** the friend plane wedged because it was serial lockstep over a lossy live-only transport with ~14 patchwork recoveries whose terminal state was a destructive re-key; the fleet plane converges because it's a CRDT. Phase A gives the friend hop the same laws (single writer per lane, idempotent ingest, heads-compared-on-every-edge, repair-is-convergence).

**How to apply / next:** Phase A field-verified across the 2026-08-08/09 log rounds (plus [[lane-rotation-wedge-heal]] closing the last wedge). **B4 COMPLETE 2026-08-09**: adopt-echo kill + HealLatch (aa8ccfd), then per-key fleet.locked/released (locks commute; legacy blob unions read-only forever), merge_rosters canonical tie-break (fgtw d75a560), roster-pull backstop on the 45s refold edge (photon 5f5fe13). **B2 compat question RESOLVED**: the fold HARD-FAILS on unknown op kinds (fgtw fleet.rs OpKind::from_u8 → "bad kind" error) — so the checkpoint op is a chain-format flag-day, and **Nick approved the flag-day outright (2026-08-09: "Flag day is totally fine. I can update the fleet easy")** — no two-step tolerance rollout needed. Next: the **B1→B3 arc** (fleet reservoir via avalanche_expand_eggs → checkpoint spine on the membership chain → re-seal chain_sync/hist_page/pong tails under epoch keys), sequenced after one clean field round of the lane heal per the plan's own gate. Then Phase C (Conversations-not-contacts → groups). Plan file: ~/.claude/plans/yeah-so-it-basically-lexical-adleman.md. Related: [[re-clutch-never-store]], [[edges-not-timers]], [[self-only-removal]].
