---
name: project-clutch-token-asymmetry
description: "clutch \"unknown conversation_token\" stalls = §4.2 competing ceremony instances; one-ceremony-discipline FIX SHIPPED @ 77390ec 2026-07-24, E2E pending"
metadata: 
  node_type: memory
  type: project
  originSessionId: bf3c2e39-d57b-4469-8848-1780b1b5c927
---

ROOT CAUSE (found 2026-07-24, live repro Nick↔David + Nick↔William + Michael): the §4.2 ceremony-owner claim shipped incomplete — parking only gated the KEYGEN QUEUE.
Token derivation is NOT the bug (derive_conversation_token sorts pids, all call sites identical); the mismatch is between ceremony INSTANCES: a device with an in-flight round kept it alive after another device claimed/took over, so friends received offers/completes from multiple instances, bound their chain to one, and dropped the other's ClutchComplete as "unknown conversation_token" forever.
Three holes: (1) fleet-sync.md §4.2 "sibling DISCARDS its parked offer" never implemented; (2) takeover's "owner absent" = is_online==false, true for every sibling at boot → Nick log shows one device taking over FIVE friendships in 2s at boot; (3) takeover could fire on woven friendships.
Also the Cloudflare bill: Michael re-sent his 548KB offer 151× over the relay via the ungated queued-KEM recovery path.

FIX SHIPPED @ photon 77390ec 2026-07-24 (one-ceremony discipline):
- Contact::discard_clutch_round() canonical teardown (never touches chain state / roster_updated / clutch_keygen_in_progress)
- presence_probed flag: takeover requires a real presence VERDICT for the owner (pong or 3-timeout), not boot-default offline
- ceremony_parked_by(): owner_woven parks FOREVER; owner present or unprobed parks; probed-offline/revoked = takeover-eligible
- merge_roster_entries adopt-arm discards our held round when another device's claim lands (covers takeover-loser via the LWW gate); never bumps roster_updated
- ALL SEVEN keep-alive paths gated: online-pong offer send, fleet-grew re-arm, stall re-fire + dozed doorbell, resend_clutch_offer, addr-change re-arm, queued-KEM recovery, keygen-result drain (drops+discards a result parked mid-keygen)
- mid-ceremony new-keys offer adopts the peer's fresh round WHOLESALE (full discard + keygen re-trigger guarded on !clutch_keygen_in_progress to stop rekey ping-pong)

E2E verification pending (needs all devices on the fixed build): expect exactly one "claiming this friendship's ceremony" per friendship, zero "taking over" while all up, "CLUTCH §4.2: discarding parked round" on non-owners, zero "unknown conversation_token" at friends.
Interim user workaround (pre-fix builds): remove & re-add the friend on the stuck device.
Deferred: persistent token-mismatch should surface as "remove & re-add" instead of retransmitting forever; taking over an in-flight ceremony without a fresh instance needs §14 chain-travel.

THIRD ROOT CAUSE (proof-mismatch class) FOUND 2026-07-24 Sarah↔Nick on IDENTICAL current builds: perfect round (same token + ceremony id, offers/KEMs exchanged) still PROOF MISMATCHED because the eggs fold a device-pubkey pair and each side supplied "their device" from the PINNED contact.public_identity — pongs re-elect the pin, so a multi-device friend answering from an unpinned device desyncs ONE egg deterministically. FIXED @ b318b13: PartySlot.offer_device = verified signer of the stored offer; completion binds the OFFER pair (agreed by construction via ceremony id); legacy fallback = the pin (exact for single-device friends). BOTH sides need the build. Also that session: the §4.2 responder exception (42b2b0c) — ownership follows the friend's choice of device when their offer sits in the slot (parking deadlocked one-device-offer friends).

SECOND ROOT CAUSE FOUND 2026-07-24 (Nick↔Jennifer): the RE-send paths (retransmit_pending_clutch_proofs + resend_clutch_offer) derived the conversation token from the RAW identity seed, not identity_party_id — first-sends were correct, every retransmit rode a garbage token ("unknown conversation_token" forever; a proof lost once could never land). The field "offer storms under an unknown token" (Emma's 0d9b7fc0) were our own re-sends, NOT a stale ghost device. FIXED @ b6f18b9. Lesson: grep ALL derive_conversation_token call sites when hunting token asymmetry; the pid seam had one more unmigrated site than believed.

Related: [[project-chain-advance-desync]], [[project-fleet-braid-plane]], [[reference-log-pull]].
