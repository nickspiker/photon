---
name: project_nat_traversal_relay_gap
description: "NAT traversal punch tiers + the RELAY = a live WebSocket pipe thru a per-recipient Cloudflare Durable Object (PipeHub), shipped 2026-07-22; offline = dropped"
metadata:
  type: project
---

Punch tiers shipped (reflexive discovery, candidates, hole-punch, validated-path keepalive).
The relay (photon 323fd07 + worker 009aeb6, 2026-07-22) is a live pipe: Durable Object `PipeHub`, one per RECIPIENT device (`id_from_name` = device hex), holds a hibernatable WebSocket; a sender's signed `relay` VSF is pushed straight down it.
Offline = dropped — no R2 mailbox, no queue, no polling (the old mailbox and its `fetch` op are gone, no compat stub).
Client: the status task holds ONE WS to `fgtw.org/pipe?dev=<device>`; frames enter the receiver's `select!` tagged `RELAY_ADDR`, so the whole data plane rides the normal dispatch; `relay_reply` returns pong/ACK over the pipe; `relay_to` = the peer's device list when `validated_path` is None.
Motivating topology: a v4/v6 split (no common IP version); any pair that can't meet directly meets at the dual-stack DO.

**Why:** two peers with no direct path still need to connect live.
**How to apply:** the relay is the last tier (all ciphertext); a true peer-to-peer relay is future work under [[project_peers_are_fgtw]]; relay media is unbuilt ([[project_traversal_relay_gap]]).
