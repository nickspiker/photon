---
name: project-reserve-delivery
description: "sender-side RE-SERVE SHIPPED 97e2bcc 2026-08-20 (Nick-approved): durable row store outranks the pending list — peer's sealed per-device lane tip + anti-entropy row deficit trigger re-serving non-pending rows at original stamps; stale-offer 60s ring gate rides along; field-verify pending"
metadata: 
  node_type: memory
  type: project
  originSessionId: d83fbeaf-685c-4da4-8647-7b49de82fd2c
---

**THE delivery-model decision (Nick approved 2026-08-20): the durable store outranks the pending list.** Shipped @97e2bcc in the sync-record drain (status.rs, the Online arm's digest block — the pull-gate comment there had already promised "the fix is on the delivery side").

**The wedge it kills** (killed two calls 2026-08-19/20, and the chronic 53-buffered/4-filled stuck messages): a row implied-delivered by a FLEET ack that one peer device never received stops retransmitting forever; the peer's in-order braid gate buffers every later row (call ANSWERS ride the lane → "answered but never went active"); the peer's pong truthfully reports the hole every cycle but rearm_pending_after only revives rows STILL pending.

**Trigger + trust:** fires only when anti-entropy shows we are STRICTLY ahead in rows (they provably lack content) AND rows exist above their lane tip that are not pending. The tip rides the pong's AEAD-sealed tail under the pairwise per-device key — cryptographically stronger than the fleet-granular, eagle_time-matched ACKs the pending list trusts (MAC-in-ACK remains the separate phase-3 item). Replayed stale pongs cost bandwidth only (receiver row-store dedup absorbs; Re-ACK clears the fresh pending).

**Bounds:** 8 rows/burst oldest-first (the oldest hole holds the in-order gate shut; later holes fill on subsequent tips), 2 bursts per stuck tip value (`lane_reserve_bursts` map — sibling-lane rows above OUR lane's tip would re-serve forever otherwise: peer dedups them and dedup never advances our tip), cap resets when the tip moves. Re-serve = chain_transmit at ORIGINAL eagle_times (lane-rotation-flush semantics; in-flight guard idempotent, window paces).

**STALE-OFFER GATE (hazard the re-serve creates):** a re-served call OFFER decrypts fresh hours later and would RING for a dead call — offers older than 60s at decrypt are recorded, never rung (call_ui.rs on_call_signal; an age check on STARTING a ring, not a ring timer — edges still end rings).

Related: [[project-voice-calls]] (answers ride the lane), the receiver-side half = chain gaps arm the urgent friend history walk (@3e1cdfc — recovers CONTENT but never advances the braid position; the re-serve is what fixes the chain). Field signal: "peer N row(s) behind … re-serving" then "gap filled" within one ping cycle.
