---
name: project_nat_traversal_relay_gap
description: "NAT traversal (reflexive/punch/keepalive) + a LIVE relay pipe now both shipped; relay is a per-recipient Cloudflare Durable Object over WebSockets, not the old R2 mailbox"
metadata: 
  node_type: memory
  type: project
  originSessionId: 214fbb36-8068-4595-9dbb-870b43abd44e
---

NAT traversal shipped the punch tiers (reflexive discovery, candidate model, live hole-punch, validated-path keepalive; commits 03c759e/60bfc17/88a3b34).
The RELAY tier is now ALSO shipped (2026-07-22, photon 323fd07 + worker 009aeb6) — but NOT as the TURN-over-UDP volunteer bounce originally imagined, and NOT the R2 store-and-forward mailbox that came between.

THE RELAY IS A LIVE WEBSOCKET PIPE.
A Cloudflare **Durable Object** (`PipeHub`, one instance per RECIPIENT device, `id_from_name = device hex`) holds a hibernatable WebSocket open to each device; a sender's signed `relay` VSF routes to the recipient's DO and the payload is pushed straight down that live socket.
Offline = dropped (no R2, no queue, no storage, no polling). A DO is the only Cloudflare primitive that can hold two peers' connections — a plain Worker is shared-nothing per request.
The old mailbox (POST to R2 `relay/{recipient}/` + 5s poll to drain) is GONE; so is the `fetch` op (deleted, an old client hits `unknown_op` and fails loud — no compat stub).

Client side: the status task holds ONE WS to `fgtw.org/pipe?dev=<our device>`; each frame is injected into the receiver's `select!` tagged `RELAY_ADDR` so the WHOLE data plane — CLUTCH, ping/pong presence, chat, acks — rides the real ~900-line dispatch, no bespoke per-type relay parser.
Bidirectional via `relay_reply` (pong/ACK returns over the sender's pipe when the message came in via RELAY_ADDR). Fan-out mirrors CLUTCH's rule: `relay_to = peer device list when validated_path.is_none()`, added to Ping/Message/AckRequest. Presence flips lime-yellow (reached_via_relay, driven by the receive side), chat ACKs clear retransmit.
Removed: 5s poll task, fetch_relay_messages, split_concatenated_vsf, dispatch_relayed_clutch.

The motivating topology was the v4/v6 split (Nick v6-only cellular ↔ friend-S v4-only DSL — NO common IP version, direct socket impossible), not classic symmetric↔symmetric NAT; the pipe covers both — any pair that can't meet directly meets at the dual-stack DO.

**Why:** two peers with no direct path (asymmetric reachability or hostile NAT) now connect live thru fgtw.org's DO instead of dead-ending at a "pending relay" log. Fits [[project_peers_are_fgtw]] (though this is still the fgtw.org server, not a peer bounce). Pairs with the doorbell [[project_doorbell]] for the both-offline wake.
**How to apply:** the CLUTCH ceremony was already proven over the OLD mailbox relay; presence + chat over the NEW pipe is built but E2E-unverified (needs friend-S + Nick on 0.40.13). Still owed separately: scripts/android/dev-adb.sh silently reuses stale Rust builds (`touch src/…` forces a real recompile). A true peer-to-peer relay (no fgtw.org) remains future work under [[project_peers_are_fgtw]].
