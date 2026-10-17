---
name: project-vault-roadmap
description: Vault phasing — ring tooling, mid-session ring resurrection, GC/compaction and cross-device mesh sync wait for the device-sync phase
metadata:
  type: project
---

The dual-ring vault (ring 0 = XDG config, ring 1 = XDG data; open-with-repair, randomized dual-write, `degraded()`) shipped 2026-06-08 and now lives in the kete crate (photon re-exports it from `src/storage/mod.rs`). The degraded/data-lost banners exist ([[project_vault_seal_failures]]).

**Deferred to the device-sync phase:** ring health tooling beyond `vaultinfo`; mid-session ring resurrection (a failed ring stays dropped until restart); garbage collection / compaction; cross-device mesh sync.

**Why:** all four need one richer ring/vault tooling layer, cheapest to build once alongside device sync.
**How to apply:** vault work = yes if it unblocks current photon flows; defer if it's ring tooling / GC / sync prep. Retention design: [[project_retention_winnow]].
