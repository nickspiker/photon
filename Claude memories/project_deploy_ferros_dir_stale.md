---
name: project-deploy-ferros-dir-stale
description: 2026-11-22 v156's ferros kist capsule was built from a STALE /mnt/Harbor/Code/ferros (HEAD 163fc220, 11-20), not the ferros forge; check FERROS_DIR before every deploy
metadata:
  type: project
---

Photon's deploy (scripts/lib/ferros-kist.sh) freezes ferros's `std/photon` from `FERROS_DIR`, which defaults to `../ferros` = /mnt/Harbor/Code/ferros. On 2026-11-22 that checkout sat at 163fc220 (2026-11-20, last reflog entry) while the ferros session committed to its forge elsewhere. So v156's capsule (on R2, and stowed on the Pro) carried 11-20's std/photon, without std/photon 69bb2528 (the `set_host_numerals` call). I first told ferros it was included, reasoning from timestamps alone; `git merge-base --is-ancestor 69bb2528 HEAD` in ../ferros showed it wasn't.

**Why:** a capsule that builds cleanly can still carry a days-old std/photon; nothing in the deploy names the ferros commit it froze.
**How to apply:** before a deploy, ask the ferros session for its forge's std/photon commit, and confirm ../ferros contains it (`git -C ../ferros merge-base --is-ancestor <c> HEAD`) or set FERROS_DIR to the forge. OPEN: where the forge lives, and whether ../ferros gets updated or FERROS_DIR changes (ferros's call).
