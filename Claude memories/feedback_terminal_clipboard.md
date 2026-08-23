---
name: feedback-terminal-clipboard
description: Format dev-mode log output so double-click word selection in a terminal yields a clipboard-clean value
metadata: 
  node_type: memory
  type: feedback
  originSessionId: d48050a2-34aa-4416-9f29-d6875b3be60b
---

When formatting `name=value` pairs in dev-mode log output, put spaces around the `=`: `name = value` (not `name=value`).

**Why:** double-click word selection in a terminal treats `=` as a word character, so `identity_seed=898354b...` selects as one token. Pasting it into `vaultinfo` (or any tool expecting raw hex) fails because the prefix isn't hex. Spaces around `=` make the hex selectable on its own.

**How to apply:** any log line meant to be copy-pasted by the user — keys, hashes, IDs, paths, anything `=`-delimited — gets spaces around the delimiter. Same applies to other "clickable" terminal output: separate the human-readable label from the machine-consumable value with whitespace.

**Fenced code blocks carry a trailing newline (Nick, 2026-08-23):** the Claude Code extension's copy includes the fence's terminating newline, so a copied "one-liner" pastes with a stowaway Enter — auto-executing in terminals, blank-lining in compose boxes. Paste-able one-liners go in INLINE code (single backticks, copies as exactly its characters); fenced blocks only for genuinely multi-line content.
