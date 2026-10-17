---
name: project_self_message_vanish
description: "CLOSED 2026-09-02: self-message latency + restart vanish = ~300 durable commits per send + quit killing queued writes; delta gate + drain_durable_writers + batch commits fixed it"
metadata:
  type: project
---

Field day 2026-09-02: notes-to-self took 40 s to minutes and a row vanished on restart.

**Convicted + fixed (photon 68b1912):**
- `save_messages` re-put EVERY row every save (fresh-nonce AEAD defeats identical-overwrite skip) → ~300 commits per send. Delta gate in `save_messages` / `save_messages_page`: skip rows whose would-be Record equals the decoded durable row.
- Quit killed queued writes. `durable_pending` counts message+chains writer enqueues; `drain_durable_writers()` blocks the quit / non-resident close / update re-exec edges until zero.
- Engine batching: manifestus `apply_batch` (one spine commit), kete librarian drains every queued mutation into one batch, rarangi transact = one ordered batch. One send ≈ 2 commits.
- PT frozen-address fix: `retarget_peer` on verified pong (photon 115a346).

The zero-remote design is correct: a self send's vault write IS delivery, same machinery as friends.
A full local wipe the same night lost nothing — FLEET-HOLDS-HISTORY backfilled every page.

Ghost device 1be949c1 is tracked in [[project_clutch_completion_rebroadcast]] / docs/ghost-device-supersession.md.
