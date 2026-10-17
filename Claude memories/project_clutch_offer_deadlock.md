---
name: project_clutch_offer_deadlock
description: "CLUTCH offer-loss deadlocks (06-28 → 09-08), all fixed: PT retries = consecutive no-progress, offer re-fire on path edges, provenance = device key + pinned send-time (no torching), ftip fold-freshness tripwire, relay sentinel survives pong salvage"
metadata:
  type: project
---

CLUTCH parked in Pending whenever an offer transfer died, because offer sending was one-shot + pong-driven. Rules the generations left:

- **PT stale cap counts consecutive NO-PROGRESS rounds, not lifetime timeouts** (7d5e356, 2026-07-19): RTO recomputes on every ACK, so a 256-packet blast on cellular self-killed a 549 KB offer on a working path.
- **Parked offers re-fire on the path-validated EDGE** (`resend_clutch_offer`) plus a stall net (`Contact::clutch_offer_stall_cycles`, every 6 validated cycles); presence pings also target `validated_path`; pong-drop gates log their reason.
- **Provenance = (device key + PINNED send-time `clutch_round_started`)**, stamped into the offer's creation_time; `CeremonyId::derive` sorts both → identical id on both sides (9a9d2d5, domain `PHOTON_CLUTCH_OFFER_PROV_v2`). Never torch/re-key on a proof mismatch: over a relay both sides torched faster than the RTT and stayed a generation apart forever. A surviving mismatch = a real bug, logged.
- Older fixes still load-bearing: PT inbound drain is stream-scoped (918552f); dual-stack sockets need V4 mapped to ::ffff: (e39e878).
- **Fold-freshness tripwire (2026-09-08):** sealed pong tails carry `ftip` (our membership-chain tip); a friend claiming a newer tip than our adopted fold triggers one 60 s-throttled refetch. Unknown-device ping drops now log (capped).
- **The RELAY_ADDR sentinel survives pong salvage**, else relay-alive contacts showed green "direct via WAN".

**How to apply:** `photonlog --pull --handle <peer>` ([[reference_log_pull]]); PT: lines tell the whole transfer story; one-sided pongs between a device pair usually mean that side's fold of our fleet is stale.
Related: [[project_clutch_token_asymmetry]], [[project_clutch_completion_rebroadcast]].
