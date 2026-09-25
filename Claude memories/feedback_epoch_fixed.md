---
name: feedback_epoch_fixed
description: HARD: the Eagle epoch is 1969-07-20T20:17:48 TAI forever — never propose, "open-question", or hedge on moving it (Nick 2026-09-25, "it's a nope")
metadata:
  type: feedback
---

The Eagle epoch is fixed at 1969-07-20T20:17:48 TAI (an integer TAI second, so Eagle second boundaries coincide with TAI/GPS/UTC). Photon has written Eagle stamps into messages, vaults and files for a long time on this epoch, and vsf + nunc are pinned to it (LOCK stage 1).

**Why:** Nick, 2026-09-25, on a Tukutahi draft whose §16 floated 20:17:46: "we're not changing the epoch, not sure why other claude thought it would be cute to sneak that maybe in there when it's a nope."

**How to apply:** treat the epoch as a constant of nature. Any spec, draft or doc that lists the epoch as an open question gets it marked DECIDED, never re-argued. Related: [[project_wave_canonical_container]], docs/lock.md, docs/tukutahi.md.
