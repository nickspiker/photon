---
name: feedback_epoch_fixed
description: HARD: never move the Eagle epoch (no 20:17:46 etc. — Nick 2026-09-25 "it's a nope"); NOTE the fleet mints on the LEGACY label 20:17:40 UTC (POSIX), the TAI definition 20:17:48 TAI runs exactly 29 s ahead and DECIDED 2026-09-26: NO flip — stamps stay on the legacy scale
metadata:
  type: feedback
---

The epoch is never moved to a different instant (a Tukutahi draft floated 20:17:46 TAI; that is a nope).
Two scales exist and must not be confused (corrected 2026-09-26 — the first version of this note got it wrong):
- LEGACY, what every stamp the fleet mints today is: POSIX seconds counted from the label 1969-07-20T20:17:40 UTC (`vsf::EAGLE_EPOCH_UNIX_SECS`, `eagle_time_now`, nunc, photon's TrueClock). Messages, vaults and files have used this for years.
- LOCK/TAI definition: 1969-07-20T20:17:48 TAI (`EAGLE_EPOCH_TAI_SECS`, `from_tai_ns`), an integer TAI second so Eagle second boundaries sit on TAI/GPS/UTC second boundaries. It runs exactly 29 s ahead of the legacy count for any instant since 2017 (`LOCK_MINUS_LEGACY_SECS`). Nothing mints on it yet.
DECIDED 2026-09-26 (Nick: "I don't want to change anything, the way it was was fine"): stamps STAY on the legacy scale — no +29 s flip. The TAI helpers in vsf are conversions only; nothing mints on them.

**Why:** Nick, 2026-09-25, on a Tukutahi draft whose §16 floated 20:17:46: "we're not changing the epoch, not sure why other claude thought it would be cute to sneak that maybe in there when it's a nope."

**How to apply:** treat the epoch instant as a constant of nature — a draft that lists moving it as an open question gets it marked DECIDED. The legacy→TAI flip is also closed (no flip); don't reopen it. Related: [[project_wave_canonical_container]], docs/lock.md, docs/tukutahi.md.
