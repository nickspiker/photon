---
name: no-wrapped-comments
description: "ALL of Nick's repos: comments are one line per thought, never hard-wrapped — applies at write time; enforced by comment-gate.sh at build"
metadata:
  type: feedback
---

Never hard-wrap comments at an arbitrary column in ANY of Nick's repos (photon, fgtw, vsf, fluor, …): one (possibly very long) line per sentence/thought; the editor soft-wraps.

**Why:** called out four+ times (2026-07-30, 2026-08-01 — "so we're going to do this all day and then write a script because you just can't seem to do it correctly?"). Writing wrapped prose and de-wrapping later is the failure mode; the rule applies AT WRITE TIME.

**How to apply:** every comment sentence is a single line (`///`, `//!`, `//` alike). Enforced by scripts/lib/comment-gate.sh (run by dev.sh via desktop.sh; photon src + the fgtw path dep): fails the build on a >60-col comment line ending in a word character whose next same-marker line starts lowercase. fgtw-bootstrap is legacy-wrapped: don't churn it, don't add new wraps. Full rule and framing: [[feedback_no_comment_wraps]].
