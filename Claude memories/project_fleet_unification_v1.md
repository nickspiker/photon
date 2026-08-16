---
name: project-fleet-unification-v1
description: fleet unification v1 SHIPPED 2026-07-25 (photon b231592) — compose ANYWHERE via fleet-forward, chain owner transmits; full §14 plane still next stage
metadata:
  type: project
---

Fleet unification v1 SHIPPED 2026-07-25 @ photon b231592 (row-sync plane, pre-§14):
- chain_transmit = the extracted WIRE half of a send (weave, braid advance, chains persist, PT dispatch, NO row bookkeeping); send_chain_message rides it + inserts its bubble.
- Compose ANYWHERE: a device with no local chain inserts the bubble (delivered=false) and pushes the row thru sibling sync; the device holding the woven chain drains fresh sibling-merged outgoing+undelivered rows onto the braid with their ORIGINAL timestamps (one row identity fleet-wide → friend dedup/re-ACK/delivered-upgrade all cohere). v1 assumption: ONE woven chain per friendship.
- Compose box shows on any device for friend convos with history while a fleet exists.
- Both fleet gates that silently killed sync are DEAD: live push unconditional (d73c223), pull-sweep sibling pick unconditional w/ relay fallback (648791b), periodic ~5min jittered sweep backstop in tick.
- Presence salvage (a376a17): unmatched-but-signed pongs count as liveness (no addr adoption).
Full §14 plane (fleet reservoir, checkpoint spine, LINEARIZER = concurrent multi-device chains) = the NEXT stage; v1's single-owner transmit is the interim.
