---
name: project-deploy-ferros-dir-stale
description: 2026-11-22 v156's ferros kist capsule was built from a STALE /mnt/Harbor/Code/ferros (HEAD 163fc220, 11-20), not the ferros forge; check FERROS_DIR before every deploy
metadata:
  type: project
---

Photon's deploy (scripts/lib/ferros-kist.sh) freezes ferros's `std/photon` from `FERROS_DIR`, which defaults to `../ferros` = /mnt/Harbor/Code/ferros. On 2026-11-22 that checkout sat at 163fc220 (2026-11-20, last reflog entry) while the ferros session committed to its forge elsewhere. So v156's capsule (on R2, and stowed on the Pro) carried 11-20's std/photon, without std/photon 69bb2528 (the `set_host_numerals` call). I first told ferros it was included, reasoning from timestamps alone; `git merge-base --is-ancestor 69bb2528 HEAD` in ../ferros showed it wasn't.

**Why:** a capsule that builds cleanly can still carry a days-old std/photon; nothing in the deploy names the ferros commit it froze.
**Resolved 2026-11-23 (the owner's one-copy rule, [[feedback_no_branches]]):** the farm worktree is retired and /mnt/Harbor/Code/ferros, the owner's single checkout, is at origin/forge, the one copy every agent commits into. So `../ferros` is right again.
**How to apply:** before a deploy, check `git -C ../ferros status -sb` reads `forge...origin/forge` with nothing ahead or behind; if it trails, ask the ferros session to bring it up before deploying. OPEN for the owner: re-publish v156's R2 kist capsule built from current forge (re-signs the release manifest; an outward publish, the owner's call).
