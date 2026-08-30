---
name: project-call-no-ring-incident
description: "2026-08-29 23:48 call from Emma rang nobody — offer DELIVERED but dropped as \"No friendship found for conversation_token\"; identity-era split across both fleets"
metadata: 
  node_type: memory
  type: project
  originSessionId: 9486060b-4dcb-4d7e-be00-c2a128f2d9f5
---

Emma dialed Nick 2026-08-29 23:48:52 (call id 18f6dce3, from her Android dev 8b29d719); NO device rang.
The call offer rides a CHAT/chain row (tok 8fc0640e) — it WAS delivered (relay: "delivered for 90e571bf" and "delivered for cacbc223", plus LAN twin frames), so delivery is NOT the failure.
Both live Nick devices logged `CHAT: No friendship found for conversation_token 8fc0640e` on every arrival and silently dropped it — no decrypt, no row, no ring. Same class as [[project_clutch_token_asymmetry]] (§4.2 competing ceremony instances).

**Why:** the fleets are split across an identity-seed derivation era.
- Nick's phone re-attested as dev cacbc223 with new-era handle_proof 7ff3835f; the fleet/rosters still carry old phone id 1be949c1 (all sends to it: "no mailbox... frame discarded"; phone's own HISTORY requests rejected "no key / unknown device / not mutual" — 404 times in one desktop log).
- Nick's desktop knows cacbc223 as a sibling ("own-handle device cacbc223") but treats proof 7ff3835f as a FOREIGN contact: parked CLUTCH ceremony, doorbell "rang 7ff3835f — fcm" every ~5 min for hours — the desktop is doorbelling Nick's own phone as if it were a stranger.
- Emma's fleet is split the same way: her phone 8b29d719 = new era (log tag b6ede618), her other device c9b5f417 = old era (log tag 21fc18c7). Nick's desktop relays happily with c9b5f417 (old friendship) but can't decode frames from 8b29d719 (new-era friendship key-set Nick never completed).
- Emma's phone: "pong answered by cacbc223 but we pinged 90e571bf", "unmatched pong from c9b5f417" — device-id confusion visible on her side too.

**LAN display:** Emma and Nick genuinely share subnet 192.168.1.x (Emma phone .163 proof 8d2d1b2b, Nick phone .170, Emma other dev .154 proof 5fb63507) — the LAN badge itself is real, the identity mapping behind it is the mess.

**2026-08-30 follow-up (stuck chats both directions):** Nick's phone (cacbc223 = the .170 device, broadcasts proof 7ff3835f; MacBook fe46a74b at .161 broadcasts the SAME proof — so 7ff3835f IS Nick's current handle_proof and the DESKTOP is the odd one out, treating its own fleet's proof as a foreign contact). Nick→Emma: phone has NO local friendship chain (pruned) → "fleet-forwarded to the chain-owning sibling" but the forward landed only on the sleeping MacBook (desktop "unreachable" per its rotation, 1be949c1 dead) → never transmitted → no ACK. Chain replication can't refill the phone because sibling HISTORY requests come back "rejected (no key / unknown device / not mutual)" — the roster knows 1be949c1, not cacbc223. Emma→Nick: still token 8fc0640e, relay-delivered to 90e571bf+cacbc223, both drop "No friendship found"; her direct PT to .170 gives up after 5 retries, retransmit GAVE UP after 8 attempts. Fleet-key migration itself (carrying establish 0e9cb1d, cutover 9faed92) is BUILT and worked (ira fan-out recovery, key unchanged rev 436) — the gap is friendship-chain restore + sibling trust of a re-attested device id.

**Root ID'd 2026-08-30:** the desktop's own proof IS 7ff3835f (own-handle beacon match uses session.handle_proof and it matched) — the "foreign contact 7ff3835f" is the SELF-CONTACT (self-is-a-contact doctrine); the desktop's SELF-conversation ceremony is the parked one (offers chase the stale device list), the 5-min FCM doorbell is its re-fire; the phone rebuilt its self conversation at 13:25 (ceremony completed), the desktop never did. The device-id change is the true root: fgtw::derive_device_keypair = blake3(ANDROID_ID) and nothing else, so 1be949c1→cacbc223 means ANDROID_ID itself changed (signature-change reinstall or factory reset — check whether the keystore/.p12 signing fix forced an uninstall). New device_secret → NEW device-vault filename → the old ring became a census stray and was DELETED (the fleet-key-redesign DANGER memory realized). Consent-only chain (bilateral Add, no exceptions) means cacbc223 was never folded → siblings reject its HISTORY requests (unknown device / not mutual) → chain replication can't refill → every downstream symptom.

**Corrections from Nick + MacBook log (2026-08-30):** 1be949c1 = Nick's OLD Android, dropped in the ocean — permanently enrolled (self-only removal, can't sign departure), permanent fan-out target/noise, standing testbed for a dead-device policy. cacbc223 = the NEW phone, properly ADDed. MacBook (fe46a74b) runs v0.66.1 (3 releases behind), sleeps constantly; the phone's 03:01 fleet-forward "delivered" into the MacBook's relay pipe ~2 min AFTER its last log line (asleep) — relay pipes have no mailbox, so the forward evaporated; no CHAT processing on wake at 03:17. MacBook also pings a contact with device id 00000000 and marks "0000000000000000 offline" (zero-device contact bug). Fleet-forward is at-most-once into possibly-dead pipes — needs durable re-serve semantics like normal rows.

**How to apply:** any "peer's messages/calls silently ignored + parked ceremony + doorbell loop + HISTORY rejected not-mutual" = era-split friendship, not transport. Fix direction is a judgment call for Nick: reconcile the era (re-clutch per [[re-clutch-never-store]]) + purge stale device ids (1be949c1), and make "No friendship found" surface loudly instead of silently discarding.
