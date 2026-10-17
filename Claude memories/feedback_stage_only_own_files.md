---
name: feedback_stage_only_own_files
description: "HARD: stage ONLY the files you changed, by path — never `git add -u` or `-A` in a shared tree (2026-09-26: `git add -u` in vsf swept another session's half-done refactor into a pushed commit)"
metadata:
  type: feedback
---

The Code tree is shared by several concurrent sessions (and Nick); any tracked file can hold someone else's uncommitted work.

**Why:** 2026-09-26 a vsf commit used `git add -u`, swept in another session's in-progress EWE refactor without its untracked ewe.rs, and pushed — vsf main no longer compiled from a clean checkout.

**How to apply:**
- Before every commit run `git status --short` and stage by explicit path only the files this session edited; `git add -u` / `-A` are banned in photon AND every sibling repo.
- If a file you need to commit also carries foreign hunks, stop and ask. Before pushing a sibling, `git show --stat HEAD` must list only your files.
- The other half: once Nick approves SHIPPING foreign work, commit it as its own commit right away — builds read the working tree, so uncommitted shipped bytes mean the repo doesn't hold what devices run.
Related: [[feedback_commit_all]] ("commit" means all of MY modified files, not the tree's).
