---
name: project_bridge
description: "BRIDGE = passless remote shell between fleet siblings riding the regular chain (docs/bridge.md); headless lifeline shipped; PIGEONS v2 (self-describing PT chunks, repair asks) built 2026-10-07, field verify pending"
metadata:
  type: project
---

**Canonical docs:** docs/bridge.md (laws, planes, future verbs), docs/headless-lifeline.md (`photon --lifeline`), docs/PT.md ("Spooled receive", "Pigeons, v2", "The bitmapless transfer").

**Model (shipped 2026-08-22):** chat-as-shell — bare commands are ordinary messages in the per-sibling conversation (retransmit/ACK/re-serve/dedup), typed `RefKind::BridgeOut`/`BridgeReset`, an off-thread persistent bash per sibling, first-receipt-only execution, works over relay. No command timeout; interactive programs wait until Stop (PTY tier is the fix).
Output streams as DELTAS (only what's new; host spool ≤64 KB, ≤1 Hz, ONE delta in flight gated on its own ACK; a refused send puts the spool back — flow control never drops transcript bytes; "finished" is a field on the last delta).

**Two hard-won invariants (field-burned 2026-08-22):**
1. Ephemeral rows REQUIRE anchor-only lanes — woven strands against rows that were never persisted = "braid strand miss", frames held forever.
2. NEVER bare-clear pending_messages — each frame links the prev hash, so a hole gap-buffers the peer forever. The sanctioned abandon is `rotate_our_lane` ([[lane-rotation-wedge-heal]]).

**Pigeons** (drop a file on a bridge conversation → lands in the host shell's cwd): `send_bridge_pigeon`, `RefKind::BridgePigeon`, `pigeon_chunk` PT frames under the FLEET key, host `network::pigeon::PigeonReceiver` zero-bitmap spool, `land_blob` never overwrites, `BridgeCwdMap`; progress bar via signed `pigeon_ack` (thinned by `pigeon_ack_due`).
v2 (2026-10-07, after chunks beat their announcement and were dropped): chunks self-describe (`sz` + sealed name `snm`), host repair ask `pigeon_want` on resume + stall, client keeps its copy until `pigeon_ack` got = of, spool records `from` → `rediscover()` at session start.
PT send window (2026-09-21, a 345 MB pigeon killed itself): `STREAMS_IN_FLIGHT = 13` per peer, per-peer stream counter mod 26, pending FIFO released in `tick()`.

**Gotcha:** the status handler in ui/photon_app/status.rs holds `checker` borrowed for its whole length, so every arm uses FIELD-level borrows — a `&mut self` method call there is a borrow error; inline it.
**Open:** field verify of pigeons v2 + the progress bar; X-death E2E of the lifeline; PT `ReceiveBuffer` spool-backing (stage 4) deferred until pigeons are field-proven. Related: [[project_unattended_reboot]].
