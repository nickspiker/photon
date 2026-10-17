---
name: project_lockout_enforcement
description: "Lockout (now the REVOKE verb) SHIPPED 2026-08-14/15: presence gate, self-go-dark, fold-authoritative identity, worker lock that survives a wipe, monotonic op guard, handle-confirmed UNLOCK via tombstone"
metadata:
  type: project
---

Field 2026-08-14: "this machine should be locked out but friends still show online" — lockout was one-directional. Shipped (photon b75cc0e / 8e02ac4 / 0f76044, worker deployed):
- **Presence gate:** `Contact::answerable_pubkeys()` drops locked_out + refused devices; `apply_locked_set` reseeds immediately.
- **Self-enforcement:** a device that finds its own key in fleet.locked clears the session and drops to Launch — it goes dark.
- **The fold is authoritative for public_identity**; the unverified peer store is a fallback only when the chain is unreachable (a self-signed PeerRecord under a scraped handle_proof can't win).
- **Worker-authoritative:** a wipe erases the local locked cache, so fgtw-bootstrap holds a `device_lock/` index and the announce refuses a locked device ("device_locked" → LaunchState::Locked dead-end, binds nothing). Lock/unlock auth = signed by a current fold member; a LOCKED signer can't lock or unlock; locking a current member is allowed (membership is permanent). `reconcile_worker_locks` / `reconcile_worker_unlocks` re-push on attest-success.
- **Monotonic op guard** (no wall-clock window — Nick vetoed ±10 min): `lock_seq/<device>` = last executed eagle osc; only strictly-newer frames execute.
- **UNLOCK:** a value-level tombstone (EMPTY `fleet.locked.<hex>`; pubkey_set_union drops len≠32) — set subtraction would make lock→unlock→relock read as unlocked. `apply_locked_set` clears locked_out ONLY on an affirmative tombstone (absence = boot lag, never forgiveness). It fires inside a handle-confirmed attest; the locked device itself is never prompted for the handle (phishing doctrine).
- Key rotations filter locked devices; locking the last other device warns (locked + lost sibling = brick until custodian supersession).
- `clear_handle_for_reproof` wipes the launch textbox at every re-proof gate (it used to auto-fill its own answer).

**Why:** a stolen device must go dark everywhere, survive a wipe, and never free itself.
**How to apply:** "Lock out" is now the REVOKE verb ([[project_lifecycle_flows]]); after rebasing onto a diverged remote, re-verify the compile.
