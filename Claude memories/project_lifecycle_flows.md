---
name: project_lifecycle_flows
description: "Lifecycle map = docs/lifecycle.md (screens, D1 KnownHandle, D2 binding marker); THE FOUR DEVICE VERBS lock/revoke/wipe/release (2026-09-08 FINAL); fleet wave lifecycle; Emma's ghost-claim fixes"
metadata:
  type: project
---

docs/lifecycle.md is THE canonical map of the identity/device lifecycle (state model, flow tree, screen names, flood conventions); the punch list is in TICKETS.
Defects it grounds (2026-07-17):
- **D1 collision:** a handle collider derives the SAME identity seed, so JoinOurs fires for strangers too. Fix = KnownHandle screen (taken-first) and NO bind request/beacon until "It's mine".
- **D2 double-attest:** device keys aren't handle-salted, so the worker's one-owner-per-device gate catches it; a client device-binding marker is consulted before spending the proof.
- D3 (last-device exit / LastRites) is SUPERSEDED by [[project_identity_never_dies]].
**THE FOUR DEVICE VERBS (Nick 2026-09-08, FINAL):**
- LOCK — self only: de-attests and nothing more (still a member, vault intact); the handle wakes it. No remote lock ("why would I want to remotely lock the device? Seems weird").
- REVOKE — treat-as-stolen: the fleet refuses it, the fleet key rotates away, the worker lock survives a wipe; refusal, never removal. Remote (Fleet row) or self (Security page, the desk case); self-revoke is refused with no other live device. Rotate the key + push the worker BEFORE `apply_locked_set`.
- WIPE — vault nuke + de-attest; stays a member.
- RELEASE — bilateral departure: sign out + wipe + leave the fleet; always frees the brand ([[self-only-removal]]).
**Fleet wave lifecycle (2026-09-08, docs/waves.md §Fleet lifecycle):** ring = breadth (the offer fans to every fold-trusted endpoint), replies = precision (the one peer device); drought → reconnecting → an honest "dropped" row; mute transmits zeros; the `fleet_wave_elsewhere` chip. Not built: join/switch handoff between siblings.
**Emma's ghost (2026-09-08):** a temp device claimed ownership of her legacy owner=None rows and LWW-pushed the claim. Fixes: a fleet-of-one can't render secured-elsewhere; a claim by a non-member is VOID on fold reconcile; a None-owner claim defers while any sibling lives.

**Why:** Nick's verbs are final; ghost claims and handle collisions were field-convicted.
**How to apply:** any new device action maps onto one of the four verbs; check docs/lifecycle.md before naming a screen.
