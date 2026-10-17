---
name: feedback-terminal-clipboard
description: "Dev log values double-click clean: spaces around `=`; paste-able one-liners in inline code, never fenced blocks"
metadata:
  type: feedback
---

In dev-mode log output put spaces around `=`: `name = value`, not `name=value`.

**Why:** terminal double-click treats `=` as a word character, so `identity_seed=898354b...` selects as one token and pastes into `vaultinfo` (or any hex tool) broken.

**How to apply:** any log line meant to be copy-pasted (keys, hashes, IDs, paths) separates the label from the value with whitespace.
Fenced code blocks carry a trailing newline (Nick, 2026-08-23): copying a fenced "one-liner" pastes a stowaway Enter that auto-executes in terminals. Paste-able one-liners go in INLINE code (single backticks); fenced blocks only for genuinely multi-line content.
