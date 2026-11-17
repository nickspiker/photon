---
name: feedback_quote_commit_ids
description: Quote a commit id only by copying it from git's own output in this turn; never type one from memory into a message
metadata:
  type: feedback
---
On 2026-11-17 I sent the ferros session five wrong commit ids (bec01aa4, d9c2fa35, 22c5f61b, b4d2f79f, a99c2c40), each needing a correction message, because I wrote the id before or without reading `git log --oneline -1`.

**Why:** a peer session builds and stows exactly the commit named; a wrong id costs a round trip at best and a wrong build on a device at worst.

**How to apply:** commit, run `git log --oneline -1` in the same command, and copy that id into the message; if the message is composed before the commit exists, name it "photon main HEAD" instead of an id.
