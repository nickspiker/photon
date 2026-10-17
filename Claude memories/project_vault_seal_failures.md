---
name: project_vault_seal_failures
description: "CONVICTED 2026-09-04: Leviathan's 43 dangling pointers = LIVE-LAP (appends over referenced blocks); lap guard + airlock doubling + prune logging + banner split shipped; the lapping path itself still unnamed"
metadata:
  type: project
---

2026-09-04 "storage degraded" on two devices. Emma's Android: one dangling pointer from an install kill mid-flush, self-healed. Leviathan: 7.5 h of `seal verification failed`, ending in a repair that pruned 43 dangling pointers. Hardware exonerated (btrfs datasum, SMART, memtest).

**Convicted:** LIVE-LAP, not corruption — the bytes were fine, the references were wrong. The mismatched pointers formed two contiguous bands = multi-block append runs written over territory the committed index still referenced. Same family as the 2026-08-24 hamt repoint-miss.

**Shipped (manifestus + kete + photon):**
1. Lap guard: `Tract::append` takes `&dyn Liveness` and refuses any referenced lba → `StorageEvent::LiveLapAverted` (error log), grow-barrier realigns, value survives.
2. Airlock grow doubles when live-heavy (the token grow re-wedged in a loop for 7.5 h).
3. kete logs every pruned pointer (lba, hash, key, route, reason).
4. Banner split: amber "storage degraded" (`vault_sick`) vs red "storage lost data" (`vault_data_lost`).
5. `vaultinfo` inspector bounded (cycle guard; no descent past a bad seal).

**Open:** WHICH path laps — watch field logs for `LIVE-LAP AVERTED at lba N`. Reproducer: `~/Downloads/corrupted vault.vsf`.
Related: [[project_manifestus_tombstone_bug]], [[project_storage_layering]].
