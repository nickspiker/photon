---
name: self-only-removal
description: "A device may only remove ITSELF from the membership chain — zero exceptions; stolen/compromised devices get LOCKED OUT, never removed"
metadata: 
  node_type: memory
  type: project
  originSessionId: faa32b1c-4aed-43dc-84b7-06eb9c63c556
  modified: 2026-08-06T17:12:30.137Z
---

Membership-chain removal is consent-only: a device signs its own departure, and nothing else may — zero exceptions (Nick, 2026-08-05: "it's critical that a device may only remove itself, zero exceptions"). A stolen, lost, or ghost device is handled by LOCKOUT, not removal: it stays a permanent chain member while the fleet stops trusting it — deaf to fleet state (key rotates away from it), cut from new braid, no delivery, frames refused.

**Why:** remote removal is the hijack vector — an attacker holding one member could evict the victim's real devices. Consent-only removal keeps the chain un-forgeable; lockout gives the fleet its defense without touching the chain (docs/device-trust-and-recovery.md "the gap" + the one-knob design: `member AND NOT refused`).

**How to apply:** never suggest or implement removing another device from the fold. Fleet-side lockout = locked-device set in fleet state + fleet-key rotation on the lock edge + trust gates exclude locked; the lock action is handle-gated (confirm de-attests, the lock fires inside the next successful attest). Friend-side = `refused_devices` fed by the reported-stolen signal in the sealed pong tail; ONE report from a trusted fold member suffices — the handle gate at lock creation is the authorization (Nick, 2026-08-06), and any threshold falls to the key-extraction tier anyway (device planting). Both shipped 2026-08-05/06. Related: [[re-clutch-never-store]].
