---
name: project-sibling-weave-trust-gate
description: "sibling CLUTCH offer dropped as \"untrusted/removed\" — the knows_device gate asks the wrong contact; a sibling can never recognize another sibling's offer"
metadata: 
  node_type: memory
  type: project
  originSessionId: a520cdd8-005f-4740-b889-3392012d4947
---

BUG found 2026-07-23 (Nick 3-device fleet on 0.43.0, new device fe46a74b "VinegarCompressor" added): sibling braid-in stuck, log spams `CLUTCH: offer from untrusted/removed device fe46a74b for d48d972b — dropping` (42×).
Root cause (photon_app.rs ~12722, offer gate; mirror gates at ~13348 KEM, ~13549 proof): the gate does `contacts.find(|c| c.handle_hash == their_handle_hash).knows_device(sender_pubkey)`. For a SIBLING offer, `their_handle_hash` = the TARGET sibling's pid (d48d972b = the phone) and sender = the new device fe46a74b. It calls knows_device on the phone's sibling CONTACT — whose public_identity is the phone's OWN key and whose fold is empty — so knows_device(fe46a74b) = false. A sibling contact only ever knows its OWN device (contact.rs knows_device: fold members or public_identity), so it can NEVER recognize a DIFFERENT sibling. reconcile_fleet_siblings DID create the contact + reseed ran; the trust MODEL is what's wrong, not propagation.
Also seen alongside: "pong dropped — responder 90e571bf is not the fe46a74b we pinged" and pongs "no pending ping matches its provenance" — same family (fleet-member cross-answer not honoured on the sibling plane).
FIX (proposed, not yet built): for a sibling target, the trust question is "is sender_pubkey ANY current member of OUR OWN fleet" (current_members / our folded sibling set), NOT knows_device on one sibling contact. Gate should special-case is_sibling: accept if sender ∈ our fleet member set. Applies to all three gates (offer/KEM/proof). The ceremony-floor gave-up latch never fired here (0×) because this isn't the Emma mismatch — it's a wrongly-rejected VALID sibling, so it never even reaches AwaitingProof.

Related: [[project-keyring-design]] (braid-in), [[project-chain-advance-desync]], [[project-fleet-braid-plane]], [[reference-log-pull]].
