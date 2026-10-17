---
name: project_identity_never_dies
description: "IDENTITY NEVER DIES (2026-07-17, SHIPPED): no terminal op; worker refuses zero-member folds; brands survive departure; two-signature release; retirement = obscure handle + puck"
metadata:
  type: project
---

Ruling (Nick 2026-07-17, superseding lifecycle D3's LastRites): **there is no way to end an identity — "how about we never."**
Any terminal op, however ceremonial, is an annihilation surface for whoever holds a device + the handle; deleting the op deletes the attack class.
Wanting out = RETIREMENT: change the handle to something obscure and park the identity on a puck device (handle-change = deferred future work).
Shipped:
- The worker refuses any fold reaching zero members; the client refuses last-device sign-out (add another device first). LastRites is cut from code and docs.
- **Brands survive departure:** sign-out ends the key's membership, never the hardware brand — a thief signing out of stolen hardware holds a branded husk.
- **Two-signature release:** the device self-signs departure, then a SURVIVING member signs `device_release` (the worker checks the signer is a current member and the released device is not). Handle knowledge alone never frees hardware.
- Released pubkeys accumulate in the fleet-synced `fleet.released` setting (concatenated 32B) so rows drop fleet-wide.
Accepted limits: a factory reset rolls the fingerprint-derived device key (the brand deters in-app re-homing only, until PIPE); a lost device stays a harmless ghost member.
The genesis-hash pin stays (custodian supersession, [[project_total_loss_recovery]]).

**Why:** the daughter case — anyone holding a device and the handle could otherwise destroy an identity.
**How to apply:** never add a terminal identity op, a free-on-zero rule or a name-returns-to-the-pool rule; the device verbs live in [[project_lifecycle_flows]] and [[self-only-removal]].
