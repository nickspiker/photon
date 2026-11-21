---
name: feedback_no_branches
description: HARD: every repo is one straight line (main); no branches or worktrees unless actually needed; land, or bundle and delete, when work is done
metadata:
  type: feedback
---

Owner, 2026-11-21: "all the repos should be a straight line, I don't run multi-part so I have no clue why this persistently makes me wonder why work disappeared and space is taken up on my disk. NO BRANCHES! unless it's ACTUALLY needed."
**Why:** branches hide work (he can't see it on main and thinks it vanished), and every worktree carries its own multi-GB target/ that filled /home and Harbor.
**How to apply:**
- Default to working on main in the main checkout, committing straight to it after the test gate.
- An agent may use a worktree branch only for the length of its task; when it lands, rebase onto main (linear, no merge commits), fast-forward, push, then delete the branch (local + origin) and remove the worktree the same turn.
- Work that can't land yet: bundle it (`git bundle create <repo>.bundle refs/heads/<b>`, verified) under /mnt/Harbor/Code/branch-bundles-<date>/ and delete the branch; record the bundle path in the relevant memory.
- Never leave farms, worktrees or scratch clones behind; delete their target/ dirs.
- 2026-11-21 sweep: photon + 15 deps went to main only; unmerged branches are in /mnt/Harbor/Code/branch-bundles-2026-11-21/<repo>.bundle (photon ferros-tcp; fgtw-bootstrap pucks-weather; fluor ferros-first-frame-touch, ferros-frame-time, ferros-launch-touch; vsf lane-0.6; opsin raw-clips; kete overhaul; manifestus overhaul; tapuwae lanes-probes). spirix's one line is `spiral` (no main). ferros did the same (forge only; ferros-branches-2026-11-20.bundle).
Related: [[project_pipe_auth]] (ferros-tcp now lives in the photon bundle), [[feedback_builds_off_tmp]].
