---
name: project_braid_working_baseline
description: First full-stack working 2-device conversation — the braid + CLUTCH + delivery all green end-to-end as of commit 6325cd9 (2026-06-28)
metadata: 
  node_type: memory
  type: project
  originSessionId: d13fb540-da07-491c-8ecc-f4d5924d759a
---

**HISTORIC MILESTONE 2026-06-28: the braid works end-to-end on two real devices.** First time the whole stack went green. Baseline commit: **6325cd9** (the last of a 4-fix arc; see [[project_clutch_offer_deadlock]] and [[project_chain_advance_desync]]).

Proof: Nick's log from a `[]x` clean re-CLUTCH between handles "Nick" and "Robert". UI screenshot showed "CLUTCH: secured" with a full alternating conversation. Nick's log analysis:
- **6 messages received from Robert, ALL decrypt clean: zil, zilor, tera, lun, lunor, stela.** 0 VsfField parse errors. 0 garbage decrypts (every CHAIN DECRYPT raw plaintext starts [40...] = '(' = valid VSF). **No msg-3 desync cliff** — the braid advanced correctly through the whole conversation.
- 6 messages sent Nick→Robert (4,3,5,4,4,6 chars).
- CLUTCH: full 8-algo ceremony, Eggs computed, Early proof verified, Complete saved + persisted (clutch_state u3⦉2⦊).
- 0 gap-buffers, 0 "No friendship found" drops.

**The reliability layer was genuinely exercised and worked:** 6 "Re-ACKed duplicate — our earlier ACK was likely lost" events. ACKs ARE being lost on this transport; the per-message re-ACK-from-stored-ack_hash fix (662c0a6) healed every one, so the chain kept advancing instead of stalling at msg-2 like every prior run. This is the load-bearing proof that fix matters.

**The 4-fix arc that got here (all on main):**
1. 918552f — PT inbound drain stream-scoped (CLUTCH offer/KEM no longer cross-wired).
2. e39e878 — udp::send maps V4→v4-mapped-v6 (dual-stack [::] socket was dropping raw SocketAddr::V4; fixed Robert→Nick delivery AND the dropped ClutchComplete proof).
3. 28b7a37 — resume path loads messages + friendship chains, not just contact state (history survives restart; no resume-gap "No friendship found" drops).
4. 6325cd9 — AwaitingProof recovery (Complete side re-arms resend on duplicate proof; AwaitingProof side keeps re-sending while peer online).
Plus the braid itself: x-text-only chain ingredient (92d0eb9) + per-message re-ACK (662c0a6) + stall-recovery rearm (5919255).

**Caveat at capture time:** only Nick's log was pushed fresh; Robert's log on disk was stale (still AwaitingProof from an earlier broken pull). Nick's received+decrypted bubbles ARE Robert's sends arriving correctly, but a byte-level both-sides confirmation (Robert's CHAIN ENCRYPT key == Nick's CHAIN DECRYPT key per eagle_time) wasn't done — Robert's log needs re-pulling for that.

**Still nominally open / watch:** the old "braid msg-3 desync" did NOT recur here, so it's likely fully resolved by the x-text-only + re-ACK fixes — but confirm over longer / rapid-fire / crossed-in-flight runs before declaring it dead. The pre-existing unrelated test_concurrent_transfers_same_peer unit-test failure is still red (asserts immediate 2-outbound under per-peer stop-and-wait; not a real bug).
