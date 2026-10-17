---
name: feedback-perl-utf8-trap
description: perl -pi with a \x{...} wide char but no -CSD double-encodes EVERY non-ASCII byte in the file (2026-10-17 audio.rs); \x2014 without braces is "\x20"+"14"
metadata:
  type: feedback
---

2026-10-17: a `perl -0pi -e 's/…/…\x{2014}…/'` edit on src/platform/audio.rs (no -CSD) upgraded the whole buffer to characters, and the write re-encoded every original UTF-8 byte as its own character — × became Ã, ≤ became â¤, file-wide.
Repaired by decoding once and mapping chars < U+0100 back to bytes (wide chars → UTF-8); verified with a mojibake grep over the diff.
Also: `\x2014` (no braces) is `\x20` + "14" — it printed " 14" in log strings the same day.

**Why:** photon source and comments are full of ×, —, ≤, →; one careless substitution silently corrupts a whole file and still compiles.
**How to apply:** for non-ASCII replacements use the Edit tool, or `perl -CSD -Mutf8` with the literal character; after any perl/sed rewrite, grep the diff for `Ã|â€|Â` before committing.
