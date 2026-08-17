---
name: no-private-handles
description: "Never put Nick's private handles or family references in code, comments, docs, or commit messages"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: faa32b1c-4aed-43dc-84b7-06eb9c63c556
  modified: 2026-08-03T12:28:29.833Z
---

Never write private handles (a friend's handle, the user's own handle, …) or family references ("a real first name", "David") into the photon codebase — comments, docs, test variable names, or commit messages. The repo is public.

**Why:** Nick, 2026-08-03: "so you're putting private handles in a public codebase???" — field-incident comments had accumulated ~40 handle mentions; all scrubbed in 059b602.

**How to apply:** Attribute field incidents with neutral roles plus the date: "a live pair", "a field device", "a phone on cellular", "(field, 2026-08-03)". Keep the lesson and the date, drop the who. Nick Spiker as author/signer is public identity and fine. Handles in pulled logs are fine to discuss in chat — they just never land in the tree. Related: [[no-wrapped-comments]].

**The pseudonym convention (re-established 2026-08-16 after an over-scrub):** handles and real names map to STABLE ordinary first names (Nick's own test identities read as Nick; peers get names like Mary, Sarah, Jennifer, Daniel, Emma). Never robotic labels (friend-M, test-identity-Z) — memories must read like prose. The handle→pseudonym MAP is itself sensitive (handles derive identity seeds) and lives OUTSIDE this repo in /mnt/Harbor/Code/keys/claude-pseudonym-map.txt.
