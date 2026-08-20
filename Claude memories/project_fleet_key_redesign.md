---
name: project-fleet-key-redesign
description: "fleet-key REDESIGN spec'd 2026-08-20 (docs/fleet-key.md @2c81c39, Nick reviewing before build): ira-wrapped, revision-published, shrink-only mint; replaces pair-secret fanout + epoch churn after the three-key wedge"
metadata: 
  node_type: memory
  type: project
  originSessionId: d83fbeaf-685c-4da4-8647-7b49de82fd2c
---

The 2026-08-20 field wedge (rolled-back desktop: stale oracle key + old slot epoch + fresh epoch 47 = nothing readable) exposed the fleet-key design as wrong, not buggy: wraps rode wipeable pair secrets, growth rotated, and the compliance-rotation path never re-sealed the fstate slot (rotate + cache-overwrite + roster-preserve guard = deadlock nobody can read or write).

Nick's spec, written to docs/fleet-key.md BEFORE implementation (his instruction: he reads, then approves the build):
- The device ira is permanent and is the BRAND: a locked device keeps membership forever (removal would hand the thief a forgotten machine); the KEY is the only thing that moves.
- ONE fleet key wrapped per unlocked member ira; kek = blake3(ecdh(rotator_eph, member_ira_pub) ‖ identity_seed ‖ bind) — seed mixed in for harvest-hardening (Nick's ruling); identity-seed-only KEK forbidden (a stolen attested device holds the seed).
- Wraps bind kfp (key fingerprint) not revision, so grows never invalidate wraps.
- Fan-out revision = publish counter; worker monotonic guard UNCHANGED and byte-blind; one worker addition: fanout_put refuses locked signers (gap: locked devices are still members, membership check alone let a thief publish).
- GROW (add/egg/unlock) = revision+1, same key, add one wrap, no re-seal. SHRINK (lock, self-departure) = the ONLY mint, atomic: preserve-pull old → mint → wrap survivors → publish → re-seal fstate; old key dropped only after.
- DELETES: oracle recovery slot (tonight's stale-key source), pair-secret wrap targeting + "dark until egged", rotation-on-growth edges, compliance-rotation path, timered roster retry (adoption becomes kfp-edge-driven).
- Cutover: flag-day PFO0→PFO1, no read-both; worker guard addition first (backwards-neutral); first post-cutover publish is a shrink-style mint.

FIELD STATE meanwhile: desktop is born-empty + wedged (cannot read fstate slot); the MacBook holds the ONLY complete history copy — [[project-great-cleanup]] census DELETES un-migrated legacy rings, so DEPLOYING THE CURRENT BUILD TO THE MACBOOK (or Android) DESTROYS THE LAST COPY. Nothing deploys to siblings until the fleet holds redundant history again. Nick was rightly furious at the "deploying is safe" claim — wire-compatible ≠ storage-safe.
