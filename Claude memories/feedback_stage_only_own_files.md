---
name: feedback_stage_only_own_files
description: HARD: stage ONLY the files you changed, by path — never `git add -u` or `-A` in a shared tree (2026-09-26: `git add -u` in vsf swept another session's half-done EWE refactor into a pushed commit, breaking clean checkouts)
metadata:
  type: feedback
---

The Code tree is shared by several concurrent sessions (and Nick). Any tracked file can hold someone else's uncommitted work at any moment.

**Incident 2026-09-26:** a Tukutahi review commit in vsf used `git add -u`; it swept in another session's in-progress EWE integer refactor (decoding/helpers.rs, encoding/primitives.rs, the `pub mod ewe;` line) WITHOUT its untracked ewe.rs, and got pushed — vsf main no longer compiled from a clean checkout. Repaired with a follow-up commit restoring those files' index state (`git restore --source=<prev> --staged <files>`), leaving the author's working tree untouched.

**How to apply:** before every commit run `git status --short` and stage by explicit path only the files this session edited (`git add path/a path/b`); `git add -u` / `-A` are banned in photon AND every sibling repo. If a file you need to commit also carries foreign hunks, stop and ask. Before pushing a sibling, `git show --stat HEAD` must list only your files. Related: [[feedback_commit_all.md]] ("commit" means all of MY modified files, not the tree's).

**The other half (2026-09-26, same WIP):** once Nick approves SHIPPING foreign work (he chose "Ship it included" for that EWE refactor), commit it as its own commit right away — builds read the working tree, so leaving it uncommitted means dev Android/macOS ran bytes the repo didn't hold and the Mac's pull would build different vsf. Nick: "not sure why that didn't happen..." Committed as vsf b94dd4b hours after it shipped.
