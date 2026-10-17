---
name: project_manifestus_plow_reloc_refusal
description: "CLOSED 2026-09-08: amber 'storage degraded' = the plow relocation guard refusing an unrouted live block; root cause LiveSet::apply ran removed before added (manifestus f28e851); VAULT_SICK now clears"
metadata:
  type: project
---

Field 2026-09-08 (Nick's phone, a fresh vault filling from history restore): three stores refused with "relocation repointed nothing for block at 2078 — refusing to orphan a live block" → `flag_vault_sick()` → a sticky amber banner. No data was lost; the guard (manifestus hamt.rs `repair_relocs`) was right.
Root cause: `LiveSet::apply` (manifestus src/vault.rs) applied `removed` BEFORE `added`, so a block added and retired inside ONE delta (the same key written twice in one group commit — the history-restore normal case) stayed live but unrouted. Fix: added first, then removed. KATs `same_key_twice_in_one_batch_leaves_no_unrouted_live_block` + `put_then_delete_in_one_batch_leaves_no_unrouted_live_block` (tests/vault.rs).
Photon: `clear_vault_sick()` on any successful persist after a failure; a latch-raised amber clears with a `StorageRecovered` toast.

**Why:** a guard that refuses the same block repeatedly means the planner's view disagrees with the index.
**How to apply:** fix the bookkeeping order, never relax the guard.
