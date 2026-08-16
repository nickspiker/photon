---
name: no-wrapped-comments
description: "ALL of Nick's repos: comments are one line per thought, never hard-wrapped — enforced by comment-gate.sh at build"
metadata:
  node_type: memory
  type: feedback
  originSessionId: db6a8a12-db99-4e93-ae59-33a1753e3d5e
  modified: 2026-08-01T01:23:30.648Z
---

Never hard-wrap comments at an arbitrary column, in ANY of Nick's repos (photon, fgtw, vsf, fluor — vsf had its own dedicated de-wrap commit 1306ed4 back in June). The idiom is one (possibly very long) line per sentence/thought; the editor soft-wraps.

**Why:** Nick has now called this out FOUR times (2026-07-30 twice, 2026-08-01 twice — the last time after catching fgtw/src/phonebook.rs fully wrapped a day after the rule was set, mocking "so we're going to do this all day and then write a script because you just can't seem to do it correctly?"). Writing wrapped prose and cleaning it up later with a script is exactly the failure mode — the rule applies AT WRITE TIME.

**How to apply:** every comment sentence is a single line, however long (`///`, `//!`, `//` alike). This is now ENFORCED: photon's `scripts/lib/comment-gate.sh` (run by dev.sh via desktop.sh, zero baseline, covers photon src + the fgtw path-dep) fails the build on a >60-col comment line ending in a word character whose next same-marker line starts lowercase. fgtw-bootstrap is legacy-wrapped; don't churn it, don't add new wraps there either. See also [[commit-trailer-built-with]].
