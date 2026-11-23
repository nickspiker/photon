---
name: project-deploy-ferros-dir-stale
description: 2026-11-22 v156's ferros kist capsule was built from a STALE /mnt/Harbor/Code/ferros (HEAD 163fc220, 11-20), not the ferros forge; check FERROS_DIR before every deploy
metadata:
  type: project
---

Photon's deploy (scripts/lib/ferros-kist.sh) freezes ferros's `std/photon` from `FERROS_DIR`, which defaults to `../ferros` = /mnt/Harbor/Code/ferros. On 2026-11-22 that checkout sat at 163fc220 (2026-11-20, last reflog entry) while the ferros session committed to its forge elsewhere. So v156's capsule (on R2, and stowed on the Pro) carried 11-20's std/photon, without std/photon 69bb2528 (the `set_host_numerals` call). I first told ferros it was included, reasoning from timestamps alone; `git merge-base --is-ancestor 69bb2528 HEAD` in ../ferros showed it wasn't.

**Why:** a capsule that builds cleanly can still carry a days-old std/photon; nothing in the deploy names the ferros commit it froze.
**Where ferros really is (ferros, 2026-11-22):** the forge is the GitHub remote's `forge` branch; /mnt/Harbor/Code/ferros/target/farm/ferros is a detached worktree the agents keep at origin/forge. /mnt/Harbor/Code/ferros is the OWNER's checkout with their uncommitted work on top: never point a deploy at it.
**How to apply:** before a deploy, set FERROS_DIR to a fresh export of origin/forge (fetch in the farm worktree, `git archive origin/forge`, siblings symlinked as in target/farm) or to the farm worktree read-only. OPEN for the owner: (0) re-publish v156's R2 kist capsule built from forge (re-signs the release manifest; an outward publish, the owner's call); (1) point scripts/lib/ferros-kist.sh's default away from ../ferros; (2) whether the owner's checkout comes up to forge.
