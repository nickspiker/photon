---
name: self-only-removal
description: "Removal is BILATERAL since 2026-08-31 (leaver requests + surviving member countersigns — mirror of add); expulsion still doesn't exist; stolen devices get LOCKED OUT, never removed"
metadata:
  type: project
---

SHIPPED 2026-08-31 (fgtw 0ebc7de + photon 24b0e72 + worker): the leaving device signs a departure request; a SURVIVING member's user approves and countersigns (the mirror of Add's consent egg); the worker refuses new bare self-departures. The leaver completes when it sees itself de-folded.
This supersedes the 2026-08-05 "self-signed only" rule: unilateral self-remove let anyone holding one unlocked device sign it out, force a key rotation and launder the hardware for their own handle (Nick, 2026-08-31).

**Why:** both doors of membership must be bilateral or the weaker one is the attack surface. Remote removal = hijack vector; unilateral self-removal = laundering vector.

**How to apply:**
- Never remove a device without its signed request, never a departure without a survivor's countersignature. Historic bare departures in stored chains keep folding.
- Stolen / lost / ghost devices get LOCKOUT, not removal: fleet side = locked-device set + key rotation on the lock edge (handle-gated); friend side = `refused_devices`, fed by the reported-stolen signal in the sealed pong tail.
Design: docs/device-trust-and-recovery.md. Related: [[re-clutch-never-store]].
