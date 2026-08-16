---
name: connection-flow-revision
description: DONE 2026-07-31 — clutch status strings now narrate the exchange; residue is the proof-echo quieting
metadata:
  type: project
---

Nick's request (2026-07-31) to replace "braiding eggs" with an explanatory flow SHIPPED the same day: `clutch_status_detail` (src/types/contact.rs) now reads creating 8 key pairs (3 families of crypto) → sending our public keys → waiting for their public keys → locking secrets to their keys → waiting for their locked secrets → combining all 8 shared secrets → sent our proof, waiting for theirs → confirming both proofs match → testing the secure channel → secured. Nick approved the set verbatim ("I love it :)").

**Why:** describe the EXCHANGE, not internals; each "waiting" step names whose side the ball is on, so "stuck on 5/8" is a diagnosis by itself.

**Residue:** quieting the proof-echo ping-pong (tracked in TICKETS.md under the closed ticket's note).
