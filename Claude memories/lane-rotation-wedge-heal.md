---
name: lane-rotation-wedge-heal
description: SHIPPED 2026-08-09 — anchor-wedge detection + lane rotation; relay legs detached in status drains; the ACK latency was serial HTTPS awaits
metadata: 
  node_type: memory
  type: project
  originSessionId: 296bef77-7c97-45b8-8d90-dc492f93e557
  modified: 2026-08-09T08:11:52.741Z
---

Field verdict (Nick+friend-M logs, 2026-08-09): the Nick→friend-M direction was fully dead — friend-M at the ANCHOR of Nick's lane (lost/never-held lane state), every Nick frame gap-buffered "ahead of us" forever (unlinkable by hash AND undecryptable at position-0 keys), 2,913 retransmit lines overnight from the give-up → stall-recovery-re-arm(tip 0) → give-up loop. Nick's "ACKs take a second or two" was the retransmit spray + INLINE-AWAITED relay HTTPS legs (~1.2s each) serialising the status-thread drain: ACK queued 07:27:04.8, dispatched 07:27:14.4.

Shipped (photon):
- `our_lane_wedged_at_peer_anchor(tip)`: peer tip 0 + oldest EXHAUSTED pending's prev ≠ our lane anchor → wedge. Deterministic, local, edge-triggered on the sync-record pong. Exhaustion gates out healthy first bursts.
- `rotate_our_lane()`: retire dead lane + pendings, mint fresh label (same root, no ceremony, no re-key — [[self-only-removal]] and [[re-clutch-never-store]] untouched). Receiver materializes from wire label, links from ITS anchor. Sender-only, no flag-day.
- Flush after the status drain releases the checker borrow: `resend_held_messages` → `chain_transmit` rebuilds fresh frames at ORIGINAL row eagle_times → history converges on row identity (receiver row-dedupe adopts; per-lane dedupe/monotonic guards are lane-indexed so old stamps on a fresh lane are clean).
- Relay legs DETACHED (tokio::spawn) in chat/ack/history drains + PT-tick relay escalation; CLUTCH drains keep awaits (offer's relayed-count verdict is load-bearing).
- Render probe threshold 8→16ms (was 60% of fresh-session log volume; phone healthy full render is 9-16ms).

Open (ticketed): nonzero-STUCK-tip wedge needs per-lane head HASHES in sync records; friend-M never fired the digest resync walk despite being rows behind (zero requests all session) — needs a log round post-heal.
