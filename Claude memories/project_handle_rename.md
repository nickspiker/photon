---
name: project_handle_rename
description: "Handle rename (rotation) is in the TOKEN spec and WILL be needed (marriage, a regretted handle); 2026-11-03 scoped: a rename = succession + slot move, four seams, no fundamental blocker; two constraints to respect before any successor record exists in the wild"
metadata:
  node_type: memory
  type: project
  originSessionId: 17b90e8e-8b1a-448e-8ac7-22b057e9a0d7
  modified: 2026-11-03T15:00:30.367Z
---

Nick 2026-11-03: the handle is the key material (passless: identity_seed = BLAKE3(handle), handle_proof = spaghettify) but "we can also rotate it, which we'll need to allow, it's in the spec… when someone gets married or has ilike2humpalot". He only wanted to be sure the current phonebook/wire/identity formats did not make it impossible. They do not.

A rename is a signed hand-off from one identity to its successor, never a label change (passless forbids a stable identity under a mutable label). Friends never see the handle after the add (they hold handle_proof, the pinned identity key, the genesis hash, a petname), so "nobody notices" is achievable if the plumbing follows. Four seams:
1. SuccessorRecord (docs/identity-succession.md; emit side unwired, [[succession-emit-side-unwired]]) must carry the NEW handle_proof (a slot move), signed by an old-chain device; the receive path (spawn_successor_check) re-pins the genesis today and would also re-key the contact row and keep friendship/bond/history.
2. Seed indexes device→owner and plate→owner point at the old handle_proof → the new genesis would be refused `device_owned`/`plate_owned`; the worker must MIGRATE ownership on a verified successor record (never release-then-reclaim: a window for anyone holding the handle string).
3. The new slot needs a chain holding every device: a new fgtw op kind whose genesis inherits the old fold under the successor record (one op, no re-pairing).
4. The vault: identity-scope value keys and vault_key("…", &vault_seed) addresses and blob names derive from the seed → a re-home like the Android root conversion, keyed on the seed, blobs renamed.
Also: device_binding rebinds; phonebook records republish under the new proof; the new handle must be unclaimed (first-come genesis). Ships before anyone renames (an older build would see the friend vanish and a stranger appear).

CONSTRAINTS TO RESPECT NOW (cheap while no SuccessorRecord exists in the wild): (a) give SuccessorRecord a new-handle_proof field BEFORE the emit side ships (a later format change is a flag day); (b) prefer friends' rows keyed on a rename-stable identity (pinned identity key or genesis hash) with handle_proof as a field, so the rename is a field update, not a row move. This shape also answers audit CSTV-02 (seed = public function of the handle): the old handle's holder cannot impersonate the successor without an old-chain device key.

Estimate: fgtw op + record ~1 day, worker migration ~½ day, vault re-home ~1 day, friends' receive re-key ~1 day.

Related: [[project_security_audit]], [[project_android_widevine_plate]] (the root conversion is the re-home pattern), [[project_total_loss_recovery]], [[project_identity_never_dies]].
