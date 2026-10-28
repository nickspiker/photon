---
name: feedback_reload_mid_ceremony
description: "HARD RULE: never reload the desktop (dev.sh / deploy.sh) mid-CLUTCH (era split + lane fork, 2026-09-10) OR mid-WAVE (kills the wave, 2026-10-28); check the log for both before every reload"
metadata:
  type: feedback
---

A desktop reload while the desktop is mid-CLUTCH with a friend (offer / KEM / completion, ~2 quiet minutes after a boot: 35 s owner election, then the round) loses OUR side's completion while the peer completes and moves era. Result 2026-09-10: Emma on the new era, the desktop never persisted it, her copy of Nick's lane decrypted to garbage at pos 2 ("LANE FORK (single-writer!)"), every later row buffered "ahead of us", a message stuck for hours. My rebuild cadence (a dev.sh per fix, each reloading the desktop) caused two such splits in one afternoon (15:42:47, 15:49:22).

**Why:** the desktop is the fleet's ceremony owner; a completion that never persists cannot be replayed, and the peer keeps the era she minted.
**How to apply:** before ANY dev.sh / deploy.sh reload, check the desktop log for a round in flight (`CLUTCH: Sent offer / Received KEM / Awaiting proof` without a following `secret minted with friend`) and wait; batch fixes so reloads are rare; cargo check for compile checks; coordinate with any sibling session repairing eras. See [[project_clutch_completion_rebroadcast]].

**Also a live WAVE (2026-10-28):** dev.sh at 20:41:44 UTC swapped the binary three minutes into Nick's wave with a friend on cellular — the wave died and the next process recovered the orphaned spool ("the crash interrupted its keep"). I had checked CLUTCH only. Before any reload also check the desktop log's last minute for `WAVE: trace #` / `WAVE: link —` lines (a wave in progress) and wait for `WAVE: hangup_wave` (or the trace lines stopping) or ask Nick.
