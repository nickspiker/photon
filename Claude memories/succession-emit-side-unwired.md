---
name: succession-emit-side-unwired
description: Identity-succession emit side (re-found flow) is NOT wired; receive path + worker slot are done
metadata:
  type: project
---

Identity succession (docs/identity-succession.md) is fully built and wired EXCEPT the emit side.

Done and committed: the shared-crate primitive (`SuccessorRecord`, `verify_for_pin`, `ContinuityEgg`), the worker slot (`handle_succession_put`/`_get` in fgtw-bootstrap, member-gated, mirrors `fstate_put`), the client oracle (`fetch_successor`/`publish_successor`), and the photon contact RECEIVE path (`spawn_successor_check` in devices.rs → `verify_for_pin` off-thread → migrate `pinned_genesis`, clear `identity_superseded`, re-fold; triggered from the genesis-mismatch branch in protocol.rs). Also done: the compile-time sunset tripwire at `V1_FLEET_VERIFY_SUNSET = (0,70,0)` in src/network/fgtw/fleet.rs.

**Outstanding ticket — the EMIT side:** nothing BUILDS a `SuccessorRecord` (via `SuccessorRecord::new`) and calls `fgtw::fleet::publish_successor` on a re-found. It is net-new — there is no existing identity-recreation/re-found UX to hook into — and needs a product decision on when/how a user declares "I re-founded this identity." Constraint: the re-founder must still hold ≥1 OLD-chain device to sign a continuity egg. Until this ships, the receive path is inert in practice (nothing publishes a record for contacts to find).

**Why:** deliberately deferred — the receive path is the security-load-bearing half (verify against the pin, can't be forged) and was safe to ship; the emit side changes founding UX and had no natural hook yet.
