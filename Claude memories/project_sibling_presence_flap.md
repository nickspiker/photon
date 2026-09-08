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

**FIX SHIPPED 2026-09-07 (two-tier pending window, status.rs):** entries strike at PING_STRIKE_AFTER=5s (dead-device timing unchanged, one strike per entry via a struck flag) but stay nonce-matchable until PING_MATCH_WINDOW=90s; a LATE match logs "late pong matched … presence honored, address held" and restores full semantics (pending purge, strike reset, sync tail, name/pin) while address-flavored actions (adoption, PT retarget, reflexive echo, Online peer_addr) stay gated to fresh matches — replay posture unchanged. The routing half needed no fix: relay_reply already returns pongs the way the ping came. SUPERSEDED theory: no pipe loss — cross-window log comparison was bogus, delivery symmetric. Field proof pending: both devices should stop flapping and 11/12 should complete once both run the build.
**Still open:** ghost 1be949c1 purge from the roster (census hazard, see [[project_fleet_key_redesign]]); the wifi-associated-cellular-default Android split (pings answered on LAN by policy routing while status pongs ride cellular) is now tolerated rather than fixed.
