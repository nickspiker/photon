---
name: project_manifestus_custodes_split
description: manifestus = storage/manifest engine (was custodes); custodes is being reclaimed for TOKEN-recovery custodians
metadata: 
  node_type: memory
  type: project
  originSessionId: d79166a5-abe4-48c5-b486-214ed8068594
---

The content-addressed storage/vault engine was renamed **custodes → manifestus** (directory `/mnt/Octopus/Code/manifestus`). The name `custodes` ("custodians") is being reclaimed for a *separate* future crate: **TOKEN recovery when all of a user's devices are lost** — social/custodian recovery of the identity TOKEN.

As of 2026-06-14 the rename is COMPLETE, including the on-disk format: package `manifestus` (Cargo.toml `name`, dir, and `manifestus::` refs in its tests + photon's lib.rs/storage/flat.rs); photon depends on it as `manifestus = { path = "../manifestus" }`. The `/mnt/Octopus/Code/custodes` dir now holds the *recovery* crate (package `custodes`). The on-disk schema tags were ALSO bumped `custodes.* → manifestus.*` (ring.rs `manifestus.spine`; hamt.rs `manifestus.hamt`/`.lone`/`.direct`/`.furrow`) plus the matching diagnostics — done deliberately pre-publish as a one-time FORMAT BREAK (the `"RÅ<"` block magic is unchanged; schema is a VSF length-prefixed string, so old blocks now fail the `schema != SCHEMA` check → Corrupt). Existing vaults written before this are unreadable and must be cleared: the Fedora dev box's `~/.config/Photon/*.vsf` + `~/.local/share/Photon/*.vsf` were deleted; the Pixel 8 needs Clear Storage / reinstall. All 51 manifestus tests + photon build pass post-change.

**Why:** the two jobs are distinct — manifestus stores blocks; custodes(-to-be) recovers identity. Keeping the storage package named `custodes` collides with the recovery crate.

**How to apply:** the storage engine is package `manifestus` at `../manifestus`; the recovery crate is package `custodes` at `../custodes`. Don't confuse them. Orphaned-block GC for manifestus is a ferros concern (reachability / mark-sweep from live roots + atomic manifest-root swap — manifestus can't enumerate by provenance); see [[project_vault_roadmap]].

**Intent (2026-06-14):** reserve both `custodes` and `manifestus` on crates.io. Prereq: rename the storage package `custodes` → `manifestus` first (else you'd publish storage under `custodes` and leave the recovery name taken/empty). Path deps block a full `cargo publish` — reserve with minimal stub crates, flesh out later. **ferros** is coming online as the field/coordination layer driven from within the TOKEN ecosystem; custodes (K-of-N Shamir social recovery, voca out-of-band verification) is the total-device-loss recovery layer TOKEN invokes. Full stack: TOKEN (identity substrate) → Photon (messenger) → manifestus (storage) → ferros (field/GC/coordination) → custodes (human recovery).
