---
name: project-fleet-braid-plane
description: braid.md §14 fleet plane CLOSED 2026-08-18 — removal-rotates shipped, checkpoint spine built, union-merge slot superseded by FLEET-HOLDS-HISTORY (docs/durability.md), linearizer retired for per-device lanes; single-egg fstate-snapshot plan REJECTED
metadata:
  type: project
---

**REJECTED 2026-07-23, must not resurface:** sealing a friendship's live chain pads as one AEAD blob under the standing fleet key and LWW-merging it (the "one egg" plan). §14 forbids it: the standing fleet key alone must not open pre-horizon content; epochs advance per sealed checkpoint, never per message; the message substrate is grow-only union-merge, never LWW-on-blob (LWW is the roster's model only); nonces are per-message tags folded into the key seed.

**Disposition (braid.md §14 header + §14.12 are the tracker):**
- Step 1 removal rotates SHIPPED 2026-07-23: `fanout_needs_rotation(wraps, members)` = wraps > members; the shrink heal rotates to survivors, re-seals fstate, the winner rotates the avatar pin. History pages were never at rest under the fleet key.
- Step 4 checkpoint spine BUILT ([[project_fleet_epoch_arc_closed]]).
- Step 3 union-merge slot SUPERSEDED: docs/durability.md decides FLEET-HOLDS-HISTORY — content never touches infrastructure; horizon = per-conversation retention dial; crypto-shred = zeroize per-epoch hist_page seal keys behind a sibling convergence gate.
- Step 6 linearizer RETIRED: per-device lanes ([[per-device-lanes]]).

**How to apply:** fleet-shared content goes thru lanes + fleet sync + the epoch spine, never a sealed snapshot under the standing key. Ceremony ownership: [[project_ceremony_claim_first]]. Related: [[project_keyring_design]], [[project_fleet_routing_scale]].
