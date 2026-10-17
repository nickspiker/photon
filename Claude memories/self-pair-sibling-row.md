---
name: self-pair-sibling-row
description: "CLOSED 2026-08-13: own-frame echo poisoned the sibling row with OUR device, and notes-row/sibling-row ContactIds collided by construction; echo guard + pid-keyed sibling ids fixed it"
metadata:
  type: project
---

Field 2026-08-12/13 (fleet sync never completing, notes-to-self empty, a parked "sibling" ceremony doorbelling every ~5 min).

**Convicted:**
1. Own-frame echo: our own sibling offer looped back (LAN multicast loopback / relay echo) and the ClutchOffer handler adopted endpoints BEFORE its duplicate check → `active_device` = OUR device, presence lied online, ceremony offered to ourselves forever. Root class: no receive path asked "is the sender US?".
2. `ContactId::from_pubkey` collided BY CONSTRUCTION: the notes-to-self row and the sibling row for the first-met device shared one id, so the keygen drain installed the sibling re-key on the notes row.

**Fixed:** echo guard (beee387: drain guard on `sender_device`, relay self-refusal, answerable-set filter); sibling ContactId keys on the sibling pid (4417b90, domain-separated, collision test); stall re-fire gate admits siblings and relay-online (5f1535a). All field-verified.
Note: ceremony rounds are memory-only — a restart mid-ceremony forces a fresh round.

**How to apply:** every receive path must refuse our own frames before touching contact state.
Related: [[split-contacts-incident]], [[relay-asymmetry-ping-reflection]].
