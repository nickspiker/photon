---
name: project_emma_brittany_group_fork
description: 2026-11-01 group era fork (two era-1 roots, each side dropping the other) — FIXED in code: lower root wins everywhere, second join answer can't replace, lowest standing party heals with a whole era over bonds; field test pending
metadata:
  type: project
---
Emma's phone dropped Brittany's frames ("frame tagged era f6e94668 from c9b5f417 — ours 256da35c … dropped") and Brittany dropped Emma's (627942236 = 0x256da35c): two era-1 mints of one group.
Likely cause (agent analysis, mint itself not in the logs): two of the sponsor's devices each answered Brittany's Join (fleet_device_acts races on liveness) and minted their own era; install_bind_wrap replaced with whichever wrap landed last; era_superseded_by broke same-index ties by each device's genesis_osc. A group's era advert reached no repair (era_heals only knew bonds).
FIXED 2026-11-01 (docs/molecules.md §10.4 "One era per index"): molecule chains — lower lane_root wins in era_superseded_by; install_bind_wrap keeps a held era at a higher index or lower root; status era_heals_after → heal_group_era_fork (lowest standing party, one fleet device, fresh WHOLE era past every index over each member's bond; minter cuts over on first bond ACK → after_molecule_cutover).
Open: never-friended members get no heal over a bond; the Join should name the sponsor device so only it answers (wire change).

**Why:** Brittany's rows in that group never reached Emma, silently.
**How to apply:** field-test a group conversation after the deploy — look for "MOLECULE: … holds a fork" and the cutover lines; related [[project_emma_bond_era_split]], [[project-molecules-build]].
