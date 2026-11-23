---
name: feedback_no_branches
description: "HARD, every repo: ONE branch, ONE straight line, ONE checkout on disk that every agent commits into; no branches, no worktrees, no farm or export copies"
metadata:
  type: feedback
---

Owner, 2026-11-21: "all the repos should be a straight line, I don't run multi-part so I have no clue why this persistently makes me wonder why work disappeared and space is taken up on my disk. NO BRANCHES! unless it's ACTUALLY needed."
**Why:** branches hide work (the owner can't see it on main and thinks it vanished), and every worktree carries its own multi-GB target/ that filled /home and Harbor.
**How to apply:**
- Default to working on main in the main checkout, committing straight to it after the test gate.
- An agent may use a worktree branch only for the length of its task; when it lands, rebase onto main (linear, no merge commits), fast-forward, push, then delete the branch (local + origin) and remove the worktree the same turn.
- Work that can't land yet: bundle it (`git bundle create <repo>.bundle refs/heads/<b>`, verified) under /mnt/Harbor/Code/branch-bundles-<date>/ and delete the branch; record the bundle path in the relevant memory.
- Never leave farms, worktrees or scratch clones behind; delete their target/ dirs.
- 2026-11-21 sweep: photon + 15 deps went to main only; unmerged branches are in /mnt/Harbor/Code/branch-bundles-2026-11-21/<repo>.bundle (photon ferros-tcp; fgtw-bootstrap pucks-weather; fluor ferros-first-frame-touch, ferros-frame-time, ferros-launch-touch; vsf lane-0.6; opsin raw-clips; kete overhaul; manifestus overhaul; tapuwae lanes-probes). spirix's one line is `spiral` (no main). ferros did the same (forge only; ferros-branches-2026-11-20.bundle).
Related: [[project_pipe_auth]] (ferros-tcp now lives in the photon bundle), [[feedback_builds_off_tmp]].

**One copy, no second working copy at all, EVERY repo (owner, 2026-11-23: "for all repos, not just ferros. NO branches"):** "I never branch or checkout. I just work on one copy always for various reasons, space, confusion, trampling ... it smells like a bad Claude habit." ferros's shared `target/farm/ferros` (a detached worktree on the one branch, kept so agents never edited the owner's checkout) left that checkout 408 commits stale, and Photon's v156 deploy froze std/photon from it. A one-developer repo has ONE checkout and every agent commits into it: only its own files, never `git stash`, rebase around uncommitted work. No farm, no detached worktree, no export copy.
