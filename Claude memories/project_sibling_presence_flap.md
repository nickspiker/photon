---
name: project-sibling-presence-flap
description: MacBook↔Pixel sibling stuck 11/12 + asymmetric online/offline CONVICTED 2026-09-07 — relay-only path, pongs arrive but classify "unmatched (late), no addr adoption", presence flaps on timeout-vs-syncrecord; ghost 1be949c1 still in roster eating pushes
metadata:
  type: project
---

Field incident 2026-09-07 (MacBook fe46a74b ↔ Pixel cacbc223 "RespectableSquare", both logs read): MacBook showed Pixel online + sibling pair at 11/12 testing the secure channel; Pixel showed MacBook offline.

**Convicted mechanics:**
- The Pixel was on cellular IPv6 (2600::/12 traverse ACKs), MacBook on LAN: NO direct UDP path either way — everything rides the relay pipe, which works (MacBook 17KB chain payloads "delivered" via /conduit; Pixel injects 2718B envelopes from fe46a74b every ~30s; Pixel's 179B pongs "delivered" back).
- BOTH sides classify the other's pongs as `unmatched pong … (late/twin/announce; no addr adoption)` every cycle — the ping↔pong match fails over the relay (pong presumably lands outside the match window / nonce already expired), so no validated path, no addr adoption, and the ping TIMEOUT counter keeps climbing: `TIMEOUT (3 consecutive) marked offline` → later a sync record/doorbell flips `ONLINE (device up)` → flap. The two sides sit in opposite phases of that flap = the asymmetric display Nick saw.
- Sibling ceremony parks at 11/12 (Complete, !chain_woven — the channel test needs the round-trip that never validates), so the friend chains owned by the Pixel never adopt on the MacBook → "secured on RespectableSquare / syncing to this device…" indefinitely. Matches the OPEN item in [[project_clutch_offer_deadlock]] ("one peer's pongs never arrive").
- **Ghost key 1be949c1** (the phone's OLD device key, see [[project_self_message_vanish]]) is STILL in the fstate roster (fleet.name.1be949c1) and the Pixel pushes history to it every cycle: `RELAY: history to 1be949c1 failed: recipient offline` — permanent wasted sends, and it inflates "pushing to 3 sibling(s)" counts.
- Separate co-observed oddity on the MacBook: a 30s loop re-encrypting the SAME 5-char message on lane 309cd896 / friendship ae1311ac with identical eagle_time i7(2555441499479677952) for hours — the ghost-retransmit signature; possibly the un-ACKed tail of the same broken pair.

**Fix directions (not yet built):** relay-path pong matching needs a window that tolerates relay RTT (or match on nonce regardless of age and adopt liveness without addr); sibling channel test should be able to complete over the relay path; purge 1be949c1 from the roster (census/ring cleanup — CAREFUL: deploying census deletion has the fleet-key history hazard, see [[project_fleet_key_redesign]]).
