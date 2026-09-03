---
name: project_bridge
description: "BRIDGE = passless remote shell between fleet siblings riding the regular chain; SHIPPED 2026-08-22 with anchor-only frames + ephemeral rows + lane-rotation fresh session"
metadata: 
  node_type: memory
  type: project
  originSessionId: f7df2de2-8ae4-45a0-a572-2865a6e4ac5f
---

BRIDGE = passless remote shell between fleet siblings (rustdesk/SSH replacement). The 2026-07 term-frame model (BridgeHost/forkpty/term_* frames) was EXCISED — it fire-and-forgot and never delivered. The shipped model (2026-08-21/22) is chat-as-shell: commands are ordinary messages in the per-sibling conversation riding the regular chain (retransmit/ACK/re-serve/dedup), bare commands (no `$`), typed RefKind::BridgeOut/BridgeReset (never content sentinels), off-thread persistent bash per sibling (cd/env persist, 30s anti-wedge timeout), first-receipt-only execution (is_new_row gate = no replay), works over relay.

Ephemeral terminal SHIPPED 2026-08-22 (270fc46 + 4a316ca): sibling lanes are ANCHOR-ONLY (weave zero strands, need none on receive), so bridge rows are never persisted and open wipes the screen + rotates away stale in-flight frames; host mirrors on BridgeReset receipt.

TWO HARD-WON INVARIANTS (both field-burned 2026-08-22):
1. The braid weaves against prior rows — ephemeral rows + woven strands = "braid strand miss" = frames held forever (no display, no ACK). Ephemeral REQUIRES anchor-only.
2. NEVER bare-clear pending_messages — each frame links the prev hash; clearing mid-chain leaves a hole the peer gap-buffers behind FOREVER ("expected prev X — buffering (ahead of us)", zero ACKs ever after). The sanctioned abandon = rotate_our_lane ([[lane-rotation-wedge-heal]]): retire the lane wholesale, peer links the fresh lane from its ANCHOR.

DELTA STREAMING (Nick's redesign 2026-09-03, after the silent v82 deploy where the operator saw NOTHING thru 52s of cargo output): frames carry only what's NEW ("just send what's missing") — the snapshot re-broadcast + its duplication is gone. Host spools per-command unsent output (bridge_partials buffer, 64KB bound with front-trim COUNTED into the frame's elision marker), broadcasts at most 1Hz (the one granted timer), ONE delta in flight gated on its OWN ACK edge (bridge_partial_inflight eagle_time watched in pending_messages — whole-lane pending>=2 was the starver: fleet-sync chatter froze the feed). A parked/refused send puts the spool BACK — flow control never drops transcript bytes. "Finished" is a FIELD not a message: the exit rides whatever delta is last (wire bdelta u0 flag + existing bexit; old parsers discard unknown fields → pre-v82 clients degrade to replace-semantics, showing just the latest chunk). Client appends delta frames to the one command row (seq-guarded against re-serve double-append, 64KB front-scrollback trim); Stop pill stays exit-gated. Quiet window = zero frames (ten silent minutes = nothing on the wire).

Still open: interactive/stdin programs (vim/top/REPL) hit the 30s timeout — needs a real-PTY tier someday. Related: [[project_unattended_reboot]].
