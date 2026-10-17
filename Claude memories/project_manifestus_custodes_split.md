---
name: project_manifestus_custodes_split
description: "manifestus = the storage/vault engine (renamed from custodes 2026-06-14); custodes = the future TOKEN total-loss recovery crate"
metadata:
  type: project
---

The content-addressed storage engine is package `manifestus` at `/mnt/Harbor/Code/manifestus` (photon dep `manifestus = { path = "../manifestus" }`; on-disk schema tags `manifestus.*`).
The name `custodes` now belongs to the recovery crate at `/mnt/Harbor/Code/custodes`: custodian recovery of the identity when every device is lost ([[project_total_loss_recovery]]).
Stack: TOKEN (identity) → Photon (messenger) → manifestus (storage) → ferros (OS / field / GC) → custodes (human recovery).
Orphaned-block GC for manifestus is a ferros concern (manifestus can't enumerate by provenance; [[project_vault_roadmap]]).

**Why:** two distinct jobs — storing blocks vs recovering an identity.
**How to apply:** don't confuse the two packages; storage work goes in manifestus.
