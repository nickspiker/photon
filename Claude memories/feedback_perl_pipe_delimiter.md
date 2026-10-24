---
name: feedback-perl-pipe-delimiter
description: Never use | as the perl s/// delimiter when the pattern contains a Rust closure (|x|) — the match lands at file start or mid-line and corrupts the file
metadata:
  type: feedback
---

Bit three times on 2026-10-24 (attachments.rs line 6, render.rs line 1 and 184, ceremony.rs line 1): `perl -0 -pe 's|...\|c\|...|...|'` — escaped pipes inside a pipe-delimited pattern do not behave as literals reliably; the regex matched the empty string or a wrong spot and spliced the replacement into line 1.

**Why:** Rust edits are full of closures (`|c| ...`), so any pattern touching one collides with the delimiter.
**How to apply:** for multi-line Rust replacements use the Edit tool, or a `head/cat/tail` splice by grep-found line numbers; if perl is unavoidable, use `s{...}{...}` (or `#`) as the delimiter. After any scripted edit, `git diff <file> | head` before moving on. Related: [[feedback_perl_utf8_trap]].
