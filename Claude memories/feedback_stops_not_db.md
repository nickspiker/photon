---
name: feedback-stops-not-db
description: Nick thinks in STOPS (factors of 2 in amplitude), never dB — express gains/levels as stops and ×2ⁿ factors
metadata:
  type: feedback
---

Nick: "I don't do dB, I do stops."

**Why:** stops are exact powers of two — they match the integer sample pipeline (a stop = one bit-shift) and need no log-base-10 mental gymnastics; dB is an opaque scale to him.

**How to apply:** in all discussion, docs, and UI facing Nick, express gain/level/attenuation as stops of amplitude (1 stop = ×2 ≈ 6.02dB). Convert incoming dB once, then speak stops: e.g. -18dB = 3 stops down = ×⅛ = `>> 3`. Prefer power-of-two gain constants in code for the same reason. Related: [[feedback_numbers_binary_at_rest]].
