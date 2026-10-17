---
name: lane-rotation-wedge-heal
description: "SHIPPED 2026-08-09 — peer-at-anchor wedge detection + lane rotation re-serving at original stamps; relay legs detached from status drains"
metadata:
  type: project
---

Field case 2026-08-09 (Nick→Emma dead): Emma sat at the ANCHOR of Nick's lane (lost lane state), every frame gap-buffered "ahead of us" forever, 2,913 retransmits overnight; "ACKs take a second or two" was inline-awaited relay HTTPS legs (~1.2 s each) serialising the status drain.

Shipped (src/types/friendship.rs, photon_app/status.rs):
- `our_lane_wedged_at_peer_anchor(tip)`: peer tip 0 + oldest EXHAUSTED pending's prev ≠ our anchor → wedge (deterministic, edge-triggered on the sync-record pong; exhaustion gates out healthy first bursts).
- `rotate_our_lane()`: retire the dead lane + pendings, mint a fresh label (same root, no ceremony, no re-key, sender-only, no flag day). `resend_held_messages` → `chain_transmit` rebuilds frames at ORIGINAL row eagle_times, so history converges on row identity.
- Relay legs DETACHED (tokio::spawn) in chat/ack/history drains; CLUTCH drains keep awaits (the offer's relayed-count verdict is load-bearing).

**How to apply:** rotation is the healer for an unrecoverable lane — never a re-key. Design of record: docs/lanes.md (rotation also used by `cut_over_to_pending`).
