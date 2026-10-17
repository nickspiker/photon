---
name: feedback_epoch_fixed
description: "HARD: never move the Eagle epoch; the fleet mints on the LEGACY label 20:17:40 UTC (POSIX); the TAI definition 20:17:48 TAI runs exactly 29 s ahead; DECIDED 2026-09-26: NO flip"
metadata:
  type: feedback
---

The epoch is never moved to a different instant (a Tukutahi draft floated 20:17:46 TAI — Nick 2026-09-25: "we're not changing the epoch … it's a nope").
Two scales, never to be confused:
- LEGACY — what every stamp the fleet mints is: POSIX seconds from the label 1969-07-20T20:17:40 UTC (`vsf::EAGLE_EPOCH_UNIX_SECS`, `eagle_time_now`, nunc, TrueClock).
- LOCK/TAI definition: 1969-07-20T20:17:48 TAI (`EAGLE_EPOCH_TAI_SECS`, `from_tai_ns`), exactly 29 s ahead of legacy for any instant since 2017 (`LOCK_MINUS_LEGACY_SECS`). Conversions only; nothing mints on it.
DECIDED 2026-09-26 (Nick: "I don't want to change anything, the way it was was fine"): stamps STAY legacy — no +29 s flip.

**Why:** the epoch is a constant; moving it or flipping scales re-stamps years of messages, vaults and files.

**How to apply:** a draft listing the epoch or the legacy→TAI flip as an open question gets it marked DECIDED; don't reopen either. See docs/lock.md, docs/tukutahi.md.
