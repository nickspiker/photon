# The Fleet Key — ira-wrapped, revision-published, shrink-minted

Spec'd 2026-08-20 after the epoch-churn wedge (field: a rolled-back desktop held three different keys — a stale oracle copy, the old slot epoch, and a freshly-minted epoch 47 — and could read nothing).
This replaces the pair-secret fan-out + epoch-rotation design.
The prior design rotated on every membership edge, keyed wraps to wipeable ceremony secrets, and forgot the state re-seal on most trigger paths; all three defects are structural, not bugs, so the design is replaced rather than patched.

## Doctrine

The device ira is permanent.
It derives deterministically from the machine (tohu oracle), survives any wipe or rollback, and is the wipe-proof root by construction.
A locked (treat-as-stolen) device keeps its ira and keeps its chain membership forever: the ira is the BRAND, not the credential.
Removing a locked ira from the fleet would hand the thief a machine the fleet has forgotten — free to present itself as a fresh device.
Kept and locked, the hardware can never launder itself: the fleet sees that ira, sees the lock, and nothing flows.
The KEY is the only thing that moves.

## The key

ONE fleet key: 32 random bytes, minted only at genesis and on shrink (see Events).
Its public identity is the key fingerprint `kfp = blake3_derive("photon.fleetkey.fp.v1", key)` — safe to publish, identifies the key without revealing it.
Everything sealed fleet-wide (the fstate slot, the epoch-spine custody slot) seals under the current key; a reader knows a re-seal happened because `kfp` changed, and only then.

## The wraps

The key is sealed separately to every unlocked member ira.
The wrap target is the member's ira KEYPAIR — the device pubkey recorded in the genesis-verified membership chain — never a pair secret, never anything vault-resident.

KEK derivation, harvest-hardened per Nick's ruling:

    kek = blake3(ecdh(rotator_ephemeral, member_ira_pub) ‖ identity_seed ‖ bind)
    bind = handle_proof ‖ kfp ‖ rotator_ira_pub ‖ member_ira_pub

- The ECDH half means opening a wrap requires the member device's ira SECRET — a thief holding the identity seed (a stolen attested device does) still cannot open any wrap but its own device's, which is exactly the one that stops being minted at lock.
- The identity-seed half means a quantum harvester who breaks the curve still needs the identity seed — the handle-derived secret that exists nowhere at rest.
- An identity-seed-only KEK is FORBIDDEN: the stolen attested device holds the seed, and the lock would be paper.
- The bind ties a wrap to (fleet, key, rotator, recipient): a wrap binds the key fingerprint, NOT the revision, so grow publishes never invalidate existing wraps, and splicing is dead — an old key's wrap fails its `kfp` bind, and re-presenting a current-key wrap opens the key you already hold.
- The AEAD keying keeps the existing 64-byte XOF split `(aead_key, commit)`: `commit` binds the ciphertext to the exact key (the partitioning-oracle / invisible-salamander defense) and doubles as the recipient selector.
- Quantum posture, stated honestly: the ECDH half is harvestable in a post-quantum world; the seed mix is the mitigation today, and the wrap layer swaps to a KEM without touching anything else in this design if that posture changes.

## The fan-out blob (PFO1)

    revision (u64, BE) ‖ kfp ‖ rotator_ira_pub ‖ wraps[]

`revision` is a PUBLISH counter, not a key generation.
It increments on every publish — grow or shrink — and exists only so the worker's monotonic guard kills replays and orders racing writers.
`kfp` is the key-change signal: same fingerprint = same key (a grow), new fingerprint = a mint happened (a shrink) and the state slots are re-sealed under it.

## The worker

The `fanout_put` guard is UNCHANGED in semantics: device-signed envelope, signer must fold as a chain member, blob accepted only if `revision > stored`, refused as `stale` otherwise, blob replaces the slot wholesale.
The worker stays byte-blind — wraps are opaque to it, and no superset/wrap parsing is ever added (that would grow attack surface in the one component whose virtue is that it cannot see inside).
ONE addition: `fanout_put` refuses a LOCKED signer.
Locked devices are still chain members by doctrine, so the membership check alone would let a stolen device publish fan-outs (self-wrap-only, forcing heal wars); the worker already holds the lock registry for other ops and applies the same refusal here.

## Events

GROW — add-device, egg-completion, unlock:
- revision+1, SAME key, same `kfp`, wraps = existing + one new wrap for the added ira.
- No re-seal, no adoption work for existing members; the new device just finds its wrap.
- Unlock is a plain grow: the lock-time mint already denied the thief everything after the lock; the owner's recovered device reads current state with the current key.

SHRINK — lock, self-departure. The ONLY mint, and it is one atomic duty on the publisher:
1. preserve: pull the fstate slot under the OLD key (the publisher still holds it).
2. mint: fresh key, new `kfp`.
3. wrap: every unlocked member ira EXCEPT the departing/locked one.
4. publish: revision+1 with the new `kfp`.
5. re-seal: push the preserved+merged fstate under the new key, in the same act.
The old key is dropped only after step 5.
There is NO rotate-without-reseal path; the compliance-rotation entry point is deleted.

GENESIS — first device establishes revision 1 with a fresh key, wrapped to itself.

## Recovery and adoption

A wiped, rolled-back, or fresh-attested device: derive the ira from the oracle, fetch the fan-out, open its wrap, done.
No ceremony required for key access, no sibling required, no side channel.
The oracle recovery slot is DELETED — it existed only because wraps rode wipeable pair secrets, and it was tonight's stale-key source.
Pair secrets remain what they are (the chain-replication/ceremony channel); they are no longer key-distribution substrate.

Adoption is edge-driven, never timered: a reader compares the blob's `kfp` to its cached key's fingerprint; unchanged = nothing to do; changed = adopt via its wrap and read the re-sealed slots.
The roster/settings retry re-fires exactly on the key-adoption edge (the old loop burned all its attempts on a timer before the key ever landed).

Races: two publishers race revisions; the loser gets `stale`, refetches, and re-applies its delta on top (a grow-add re-applies trivially; a shrink loser adopts the winner's key and republishes only if its shrink is still unsatisfied).
Concurrent shrinks converge on the worker's monotonic guard exactly as today.

## What nothing-flows means for a locked device

- No wrap under any current or future key.
- Worker refuses its writes on every slot, fan-out included.
- Peers and siblings verify-or-withhold against the synced locked set (`fleet.locked.<ira>` per-key entries).
- Its chain membership and its lock stand forever as testimony — ostracism, not erasure.

## What this deletes

- The oracle recovery slot and its KEK (`recovery_kek`, `publish_recovery_slot`, the refresh-on-every-edge calls).
- Pair-secret wrap targeting (`fanout_seal`/`fanout_open` pair-secret paths, the "dark until egged" state, the not-yet-egged skip).
- Rotation on growth: the newly-egged-sibling rotation edge, the add-device bind rotation, the unlock rotation.
- The compliance-rotation path (rotate + cache-overwrite + no re-seal — the mechanism of the 2026-08-20 wedge).
- Epoch-as-key-generation semantics and the churn (47 epochs in three weeks of a three-device fleet; under this design that number is the count of actual evictions).
- The timered roster retry.

## Why the 2026-08-20 wedge is unconstructible here

The rollback kept the ira, so the wrap opens on the first fan-out fetch — there is no stale side-copy to mislead it, because the side-copy no longer exists.
No growth event mints, so the slot's key never moves out from under a reader mid-session.
The only mint carries the re-seal in the same act, so a readable fan-out always implies readable state slots.
And a device that cannot read the slot cannot clobber it — the roster-preserve guard stays exactly as shipped.

## Cutover

Flag-day, consistent with the no-compat doctrine: PFO1 replaces PFO0 outright, readers do not read-both.
Order: (1) worker deploys the locked-signer refusal on `fanout_put` (backwards-neutral — PFO0 blobs still pass the guard); (2) clients ship PFO1; (3) the first publish after cutover is a shrink-style mint (fresh key, ira wraps, re-seal), which retires every PFO0 artifact in one act.
The worker's stored revision register carries forward untouched — PFO1 revisions continue the same monotonic sequence, so the flag-day-deadlock class (proposing 1 over a live register) cannot recur.
