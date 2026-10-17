---
name: no-private-handles
description: "Handles are KEYS (seed = BLAKE3(handle)) — never in ANY repo, public or private; committed content uses the map's stable ordinary first names (petnames); the map itself lives in keys/ only"
metadata:
  type: feedback
---

Photon's privacy model ([[project_humanitys_code]]): EVERYTHING is public except signing keys and handles, which live only in keys/ (Chiton/MEGA mirrored + the PRIVATE github keys repo — [[reference_backups]]). A handle is a key, so a handle in ANYTHING git-tracked (code, docs, tests, commit messages, this memory corpus) is an SSN-grade leak — private repos included. A published handle cannot be un-published; the only remedy is ROTATION (the person picks a new handle).

**Petname convention (2026-08-16; do NOT drift back to "neutral roles"):** people in committed content get the STABLE ordinary first name from the map's right column — prose, never robotic labels (friend-M, "a field device" where a person is meant). When the map's right column changes, rename the stale name in place across the tree (Mary→Emma, 2026-08-23).

**Why:** Nick 2026-08-03: "so you're putting private handles in a public codebase???" (~40 mentions scrubbed, history rewritten); again 2026-08-23 when a private memory repo was found carrying four people's handles.

**How to apply:** before naming a person in anything tracked, read `claude-pseudonym-map.txt` in the keys/ checkout (desktop: /mnt/Harbor/Code/keys/; left column = handle = SECRET, right = safe name) and never copy any part of the mapping into a repo. If a handle is found in tracked content: scrub it AND tell Nick immediately so rotation can happen. Handles in pulled field logs may be discussed in conversation; they never land in a tree. Related: [[feedback_never_store_handles]].
