---
name: project_device_loaners
description: "Loaner devices DECIDED 2026-09-04: LEASE model (docs/device-lease.md) — title never moves, leased device = PROXY never member, foreign hardware never receives a handle; vocabulary BIND/UNBIND, never sign in/out"
metadata:
  type: project
---

**Canonical docs:** docs/device-lease.md (the lease), docs/key-custody.md (handle entry), docs/lifecycle.md.

**Decisions (2026-09-04):**
- Grant = owner-fleet-signed brand annotation naming the guest; worker attest rule = brand owner OR live grant; recall = owner-signed edge at the ROUTING layer (the lockout machinery pointed at a guest), never a chain op.
- A leased device is a PROXY, never a member: no genesis, no vote, no durable vault (streams from the fleet, composes thru fleet-forward). Device classes: MEMBER / PROXY / HUSK.
- Lattice: lease = friends-only; stranger = ephemeral SESSION; newcomer = NEW OWNER release (no genesis on leased hardware — sybil rule).
- The airport flow is the ONLY lease flow: the handle never touches borrowed hardware; a delegated session key (routing + fleet-streamed history, never the seed) lands instead, killable from any owned device. The typed-handle "household lease" was CUT (Nick: foreign hardware never receives a handle, family included).
- Owned hardware: DIRECT handle entry on that device is the primary flow; tap-only fleet vouch and approver-typed handles are rejected (possession alone is nothing).
- Departure asks WHAT'S HAPPENING: new owner (countersign + release), desk/wiped (countersign, brand kept), loaner. Every decision point completes or visibly holds — no flow mints a second step discovered later.
- De-attest must NOT release ownership claims (else one machine launders handle after handle); `[]x` wipe releases, app-data clear keeps.

**VOCABULARY (Nick 2026-09-07): no passwords → never "sign in/out".** Verbs: BIND/UNBIND (device ↔ fleet), attest, lock/unlock, lease/loan, release, retire. "Attested identity", never "signed-in".
**How to apply:** use these verbs in every new string; removal consent model lives in [[self-only-removal]] and [[project_device_sovereignty]]; recall announcements ride [[project_fleet_inbox]].
