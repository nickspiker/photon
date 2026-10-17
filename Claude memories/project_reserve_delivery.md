---
name: project-reserve-delivery
description: "Sender RE-SERVE (97e2bcc, 2026-08-20): the durable row store outranks the pending list — the peer's sealed lane tip + a row deficit re-serve rows at original stamps, once per row per session; stale-offer 60 s gate"
metadata:
  type: project
---

**Decision (Nick 2026-08-20): the durable store outranks the pending list.**
The wedge: a row implied-delivered by a FLEET ack that one peer device never got stopped retransmitting forever, and the peer's in-order braid gate buffered every later row (wave answers ride the lane → "answered but never went active").
- **Trigger:** anti-entropy shows we're STRICTLY ahead in rows AND non-pending rows exist above the peer's lane tip (the tip rides the pong's AEAD-sealed tail under the pairwise per-device key).
- **Bounds:** 8 rows per burst, oldest first; a re-serve = `chain_transmit` at the ORIGINAL eagle_times; `lane_reserved_rows` = one re-serve per (fid, row) per session (2026-09-08 storm fix: re-arming on every pong re-encrypted rows 29× and broke braid order); a re-serve never takes the last in-flight window slot; the duplicate path re-ACKs with a blake3(content) stand-in.
- **Stale-offer gate:** a re-served wave offer older than 60 s at decrypt is recorded, never rung (an age check on starting a ring, not a ring timer).
- Receiver half: chain gaps arm the urgent friend history walk (recovers content, never advances the braid).
- Open (2026-09-08): why a peer's pong can list no head for our own lane label (tip 0) — read the "tip 0 from <dev> — our label X vs their lane head(s)" line.

**Why:** a hole the peer truthfully reports must get filled even after the pending list forgot the row.
**How to apply:** re-serves keep original stamps and stay bounded; never let convergence re-arm on every testimony change. Related: [[lane-rotation-wedge-heal]], [[per-device-lanes]].
