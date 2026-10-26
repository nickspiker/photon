---
name: project_sibling_group_pages
description: "2026-10-25 CONVICTED + fixed (330bbc38): sibling group history pages dropped at dispatch; sibling pong records drove friend-lane rotation/clears"
metadata:
  node_type: memory
  type: project
  originSessionId: 09cee175-fdfa-4385-b01a-117eda364782
  modified: 2026-10-26T00:52:43.882Z
---

Field 2026-10-25 (umbrella molecule, Nick/Emma/Brittany on v128): stuck group photons hit only multi-device people (Brittany has one phone and got everything).

Chain: a sibling receives a group frame first and pushes its per-lane CHAIN-SYNC checkpoint; the other device's lane moves past the frame, so the live frame is "Skipping duplicate — row not held here" or "braid decrypt landed on moved lane state".
The row was meant to arrive as a fleet history page, but the HistoryPageReceived dispatch gate in status.rs required `contact_idx_for_conversation_token`, which a molecule token never resolves — "HISTORY: page from X DROPPED — token resolves to no contact (from_sibling=1)" (20-34 per device).
drain_history_pages already had the molecule branch; the dispatch gate now passes sibling pages whose token is in molecule_rosters.

Second find, same commit: the Online sync-record loop matched records by token from ANY responder; a pong tail carries the responder's records for every conversation, so a sibling's record (tip 0 for our lane, it never receives it) fired the anchor-wedge rotation every ~2 min against an offline friend (253 rotations, 502 GAVE UP lines).
Now `lane_testimony = !responder_is_sibling && !chains.molecule` gates rotation and clear_pending_up_to; rearm skips siblings only. In a molecule one member's head must never clear a pending owed to others.

**Why:** fleet + group convergence; see [[project_member_relay]], [[project_fleet_holds_everything]], [[lane-rotation-wedge-heal]].
**How to apply:** next field round, check `token resolves to no contact` and `Skipping duplicate … row not held` are gone and siblings' umbrella row counts match.
Still open: a frame tagged an era the receiver installs 50 ms later is dropped (stale/unknown era) and waits for a retransmit; could be held like unknown-signer frames.
