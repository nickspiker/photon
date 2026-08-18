---
name: project-chain-advance-desync
description: RESOLVED 2026-08-18 - the 2026-07-23 braid desync class is closed; ten-round field soak verified bidirectional messaging; wedge heals all shipped (implied-ACK, tip-clear, anchor + stuck-tip rotation)
metadata:
  type: project
---

**RESOLVED 2026-08-18.** The pinned two-sided repro (cross-probe race + re-ACK blocked) and every descendant wedge class are closed, verified by the ten-round field soak ([[project-fleet-epoch-arc-closed]] holds the full round-by-round record):
- probe rows persist ack_hash + are weave-ineligible; nonce fork-repair backstops.
- ACK delivery bookkeeping now has FOUR converging heals: implied-ACK (in-order lane: ack for T delivers everything older), tip-clear (peer's sync-record lane head is testimony), anchor rotation (exhausted at a live tip-0 head), stuck-tip rotation (2 exhaust→re-arm ladders at one frozen nonzero head).
- attempts persist (a restart never amnesties a dying lane); salvaged pongs carry sync records (the tip pipeline can't be starved by provenance races).

Soak verdict: messages fast both directions, Nick→Emma DIRECT, Emma→Nick healed via stuck-tip rotation. The one residual (relay ring desktop→friend) is network topology (inter-segment unicast filtered), not protocol.
