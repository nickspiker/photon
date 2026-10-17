---
name: succession-emit-side-unwired
description: Identity-succession emit side (re-found flow) is NOT wired; receive path + worker slot are done; no deadline since the v1 sunset was retired 2026-10-06
metadata:
  type: project
---

Identity succession (docs/identity-succession.md) is built EXCEPT the emit side.

Done: the shared primitive (`SuccessorRecord`, `verify_for_pin`, `ContinuityEgg`), the worker slot (`handle_succession_put`/`_get`, member-gated), the client oracle (`fetch_successor` / `publish_successor`), and photon's RECEIVE path (`spawn_successor_check` in devices.rs, triggered from the genesis-mismatch branch in protocol.rs).

**Open:** nothing builds a `SuccessorRecord` and calls `publish_successor` on a re-found. Net-new UX with no hook yet; needs a product decision on how a user declares "I re-founded this identity". The re-founder must still hold ≥1 OLD-chain device to sign a continuity egg. Until then the receive path is inert.

**Why deferred:** the receive half is the security-load-bearing part and was safe to ship alone.
2026-10-06: the v1 fleet-verify sunset tripwire was retired (v1 path permanent by ruling), so this is a plain design item for the total-loss / re-found story ([[project_total_loss_recovery]]).
