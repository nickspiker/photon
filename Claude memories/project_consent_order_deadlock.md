---
name: project_consent_order_deadlock
description: CONVICTED 2026-10-01 (Jeff's log) — the keygen queue ignored the consent gate (548 KB offer left 35 ms after an add), and a knock/offer arriving BEFORE we add the sender was dropped as a stranger's with no replay → ceremony never armed; fixed (queue gated on consent_mutual; stranger knock tokens remembered, an add that matches them is the mutuality edge)
metadata:
  type: project
---

Jeff (new install, fleet of one, device 7e43f101) added Nick 2026-10-01 21:51:26: knock sent ("ceremony waits for their add"), then 35 ms later `CLUTCH: spawning keygen for Pending contact` and the full 548 KB offer fanned out to every Nick device. Nick had not added him, so on every device the offer and the earlier knock were a stranger's and were silently dropped (no replay). When Nick later added Jeff, Jeff already had keys, so nothing came back; both sat.

**Two defects, both fixed 2026-10-01 (built, unverified in the field):**
1. The keygen queue (conversation.rs `next_idx`) had no consent term — the gate lived only at the pong-driven re-send (status.rs), and the keygen-result handler sends unconditionally. Now `(c.is_sibling || c.consent_mutual)` gates the queue, so a friend's keygen waits for their knock or their offer-as-evidence.
2. Ordering: a stranger's knock is dropped but its conversation TOKEN is remembered (`PhotonApp::stranger_knocks`, session-only, ≤32, FIFO); the add path (launch.rs) checks the new contact's token against it — a match sets `consent_mutual = true` ("knocked before we added them"). Offers from strangers are NOT parked (548 KB); the existing re-send recovery covers them.

**Owner race** (Nick: "the clutch assigned machine does not have Jeff as a contact"): an offer landing on the owner before the roster push adopts the new row is dropped as a stranger's and heals only thru re-send. With defect 1 fixed the offer leaves only after mutuality, which sits after the roster push; a parked-offer buffer would close it fully. (Ownership since moved to claim-first: [[project_ceremony_claim_first]].)

**Not the self row:** Jeff's log shows no notes-to-self row; self is opt-in since 9174e449. A stray self row on a test identity likely comes from the FGTW contacts backup restore at attest (launch.rs "merged N new contact(s) from FGTW") for a RE-USED handle.
