---
name: no-private-handles
description: "Handles are KEYS (seed = BLAKE3(handle)) — never in ANY repo, public or private; committed content uses the map's stable ordinary first names as prose; the map itself lives in keys/ only"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: faa32b1c-4aed-43dc-84b7-06eb9c63c556
  modified: 2026-08-23
---

Photon's privacy model ([[project_humanitys_code]]): EVERYTHING is public, with exactly two exceptions — signing keys and handles — and both live only in the keys/ directory (Chiton/MEGA mirrored, plus the PRIVATE github keys repo — [[reference_backups]]), never in any other repo. A handle is a key: seed = BLAKE3(handle), so a handle written into ANYTHING git-tracked — code, docs, tests, commit messages, this memory corpus — is an SSN-grade leak, and that holds for private repos exactly as for public ones (the retired private memory repo carried four people's handles in its history for weeks, 2026-08-23). A published handle cannot be un-published; the only remedy is ROTATION (the person picks a new handle).

**The pseudonym convention (re-established 2026-08-16 after an over-scrub; do NOT drift back to "neutral roles"):** people in committed content get the STABLE ordinary first name from the map's right column — Nick's own test identities read as Nick; peers read as their mapped first names. Never robotic labels (friend-M, test-identity-Z, "a field device" where a person is meant) — memories and comments must read like prose. When the map's right column changes, the old name is STALE and gets renamed in-place across the tree (Mary→Emma, 2026-08-23).

**Why:** Nick, 2026-08-03: "so you're putting private handles in a public codebase???" (~40 mentions scrubbed in 059b602, public history later rewritten); again 2026-08-23 when a wrong "neutral roles / private-repo-is-OK" mutation of this rule was found alongside actual handles in the then-private MacBook corpus.

**How to apply:** Before naming a person in anything tracked, read `claude-pseudonym-map.txt` in the keys/ checkout (desktop: /mnt/Harbor/Code/keys/; left column = handle = SECRET, right column = the safe name) — and never copy any part of the mapping into a repo. If a handle is found in tracked content anywhere: scrub it AND tell Nick immediately so rotation can happen. Handles in pulled field logs may be discussed in conversation; they just never land in a tree. Related: [[no-wrapped-comments]], [[push-after-landing]].
