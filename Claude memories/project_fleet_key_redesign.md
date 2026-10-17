---
name: project-fleet-key-redesign
description: "fleet-key redesign SHIPPED + CROSSED 2026-08-20 (docs/fleet-key.md): ira-wrapped fanout v2, grow = same key + re-wrap, shrink = the ONLY mint, carrying establish never mints over sealed state; field-proven v67→v71"
metadata:
  type: project
---

The 2026-08-20 field wedge (stale oracle key + old slot epoch + fresh epoch = nothing readable) showed the old design was wrong, not buggy. Nick's spec is docs/fleet-key.md (he reads the spec before approving a build).

**Load-bearing rules:**
- The device ira is permanent and is the BRAND: a locked device keeps membership forever; only the KEY moves.
- ONE fleet key wrapped per unlocked member ira; kek mixes the identity seed (harvest-hardening) but is never seed-only. Wraps bind the key fingerprint (kfp), not the revision.
- GROW (add/egg/unlock) = revision+1, same key, full re-wrap of the desired set, no re-seal. SHRINK (lock, departure) = the ONLY mint, atomic: preserve-pull old → mint → wrap survivors → publish → re-seal fstate; drop the old key only after.
- The worker's `fanout_put` refuses locked signers; its monotonic guard stays byte-blind.
- `recover_or_establish_carrying` (fgtw client): an in-place upgrade's mint preserves + re-seals with the cached old key; a device with nothing preservable REFUSES to mint over sealed state; a sole-member fleet establishes.
- Mixed-version fleets mint-war (each side sees the other's blob as absent): every device must cross the flag day.
- Fan-out mint/grow must sign with the device's full signer (`signer_for`), else the worker's envelope floor refuses it ([[project-bindreq-peer-to-peer]], 2026-10-17).

**Residue:** the 2026-08-31 list (sibling hist_page/pong-tail epoch re-seal, row-cadence mint) — code now has `fleet_epoch_seal_key`, `photon.pong.seal.sib.v1` and `ckpt_mint_due` ([[project_fleet_epoch_arc_closed]]); confirm before reopening. The v1 fleet-verify path is permanent (ruling 2026-10-06); succession EMIT stays unwired ([[succession-emit-side-unwired]]).
