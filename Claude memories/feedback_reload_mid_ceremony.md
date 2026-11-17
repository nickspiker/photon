---
name: feedback_reload_mid_ceremony
description: "Default, not a hard rule: prefer to reload the desktop when no CLUTCH round is progressing and no WAVE is live; a STUCK round may be dropped by a restart or update (owner 2026-11-17)"
metadata:
  type: feedback
---
**Owner correction 2026-11-17** (via the ferros session): "Never had a don't interrupt handshake rule ... it is beneficial in most cases to wait until that's complete to do, say a restart or update but if it's stuck then that's kinda the point." So: waiting is the sensible default for a round that is PROGRESSING, never a hard rule, and a stuck round is exactly what a restart or update may drop.

What waiting protects, from the field: a desktop reload while a friend's round was mid-completion lost OUR side's completion while the peer completed and moved era (2026-09-10, Emma: a lane fork, rows buffered "ahead of us" for hours); and a reload during a live wave killed the wave (2026-10-28).

**How to apply:** before a dev.sh / deploy.sh reload, read the desktop log in its OWN step (a check run in the same command as the reload acts on nothing): if a round is moving (offer, KEM and proof lines within the last minute or two) or a wave is live (`WAVE: trace #` / `WAVE: link —` lines), wait for it to finish; if a round is stuck (parked, "reached NOBODY", or no progress for minutes), reload. Batch fixes so reloads are rare. See [[project_clutch_completion_rebroadcast]].
