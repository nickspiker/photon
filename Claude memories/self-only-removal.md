---
name: self-only-removal
description: "Removal is BILATERAL since 2026-08-31 (leaver requests + surviving member countersigns — mirror of add); expulsion still doesn't exist; stolen devices get LOCKED OUT, never removed"
metadata: 
  node_type: memory
  type: project
  originSessionId: faa32b1c-4aed-43dc-84b7-06eb9c63c556
  modified: 2026-08-31T22:00:00.000Z
---

Membership-chain removal is BILATERAL (SHIPPED 2026-08-31, fgtw 0ebc7de + photon 24b0e72 + worker deployed): the leaving device signs a departure request (departreq_signing_bytes), a SURVIVING member's user approves on their screen and countersigns (consented Remove op — the exact mirror of Add's consent egg), and the worker refuses newly-appended bare self-departures. The leaver completes when it observes itself de-folded (wipe for Remove & shred, keep-vault de-attest for plain Remove). This supersedes the 2026-08-05 "self-signed only" rule because a UNILATERAL self-remove was itself an attack: whoever briefly held one unlocked device could sign it out — forcing a fleet key rotation and laundering the hardware into a clean re-attestable device for their own handle (Nick, 2026-08-31). Expulsion still does not exist: without the leaver's request signature nothing folds. A stolen, lost, or ghost device is handled by LOCKOUT, not removal: it stays a chain member while the fleet stops trusting it.

**Why:** both doors of membership must be bilateral or the weaker one is the attack surface — add was request+consent, exit had to match. Remote removal stays impossible (hijack vector); unilateral self-removal is now impossible too (laundering vector).

**How to apply:** never suggest or implement removing another device without its signed request, and never a departure without a surviving member's countersignature. Historic bare departures in stored chain prefixes keep folding (no flag day). Fleet-side lockout = locked-device set in fleet state + fleet-key rotation on the lock edge + trust gates exclude locked; the lock action is handle-gated (confirm de-attests, the lock fires inside the next successful attest). Friend-side = `refused_devices` fed by the reported-stolen signal in the sealed pong tail; ONE report from a trusted fold member suffices — the handle gate at lock creation is the authorization (Nick, 2026-08-06), and any threshold falls to the key-extraction tier anyway (device planting). Both shipped 2026-08-05/06. Related: [[re-clutch-never-store]].
