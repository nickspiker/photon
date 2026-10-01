---
name: project_consent_order_deadlock
description: CONVICTED 2026-10-01 (Jeff's log) — the keygen queue ignored the consent gate (548 KB offer left 35 ms after an add), and a knock/offer arriving BEFORE we add the sender was dropped as a stranger's with no replay → ceremony never armed; fixed (queue gated on consent_mutual; stranger knock tokens remembered, an add that matches them is the mutuality edge)
metadata:
  type: project
---

Jeff (new install, fleet of one, device 7e43f101) added Nick 2026-10-01 21:51:26: knock sent ("ceremony waits for their add") — then 35 ms later `CLUTCH: spawning keygen for Pending contact` and at 21:51:27 the full 548 KB offer fanned out to every one of Nick's devices (delivered to 90e571bf, fe46a74b, cacbc223; dropped for the locked-out phone 1be949c1). Nick had not added him: on every device the offer and the earlier knock were a stranger's and were silently dropped (FriendKnockReceived `continue`, no replay). When Nick later added Jeff on one device, that device's knock flipped Jeff's consent but Jeff already had keys, so nothing came back; Nick's side never saw mutuality; both sat.

**Two defects, both fixed 2026-10-01 (built, unverified in the field):**
1. The keygen queue (conversation.rs `next_idx`) had no consent term — the consent gate lived only at the wire's pong-driven re-send (status.rs ~1001), and the keygen-result handler (ceremony.rs "Send our offer if not already sent") sends unconditionally. Now `(c.is_sibling || c.consent_mutual)` gates the queue, so a friend's keygen waits for their knock or their offer-as-evidence.
2. Ordering: a stranger's knock is dropped on the wire but its conversation TOKEN is remembered (`PhotonApp::stranger_knocks`, session-only, ≤32, FIFO); the add path (launch.rs search-result) checks the new contact's token against it after sending our knock — a match sets `consent_mutual = true` ("knocked before we added them"), so the queue arms. Offers from strangers are NOT parked (548 KB); the existing deadlock recovery covers them: our queued KEM re-sends our offer on pong edges and a peer receiving a duplicate offer with its KEM already out re-sends its own.

**The owner race Nick described** ("the clutch assigned machine does not have Jeff as a contact"): the computed owner (lowest online fold device, era.rs:130) adopts the new row via the roster push ~1 s after the adding device pushes; an offer that lands on the owner before the adopt is dropped as a stranger's and heals only thru the re-send recovery. With defect 1 fixed the offer leaves only after mutuality, which sits after the roster push, so the window closes in practice; a parked-offer buffer would close it fully.

**Not the self row:** Jeff's log shows NO notes-to-self row (roster 0 before his add); the one extra conversation is the atom "zeno" he founded at 21:48:37. A fresh-identity self row is minted by no code path found (self is opt-in since 9174e449); the likely source on Nick's test identities is the FGTW contacts backup restore at attest (launch.rs ~233 "merged N new contact(s) from FGTW") for a RE-USED handle.
