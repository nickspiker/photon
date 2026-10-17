---
name: project_two_machine_git_divergence
description: "Photon is built on two machines (Leviathan desktop + MacBook) whose histories diverged 3 times; verify local == origin on the WIRE; publish commits locally, so pull --rebase then push"
metadata:
  type: project
---

Leviathan (Linux, `/mnt/Harbor/Code/photon`) and a MacBook both commit to photon and its sibling crates. Histories silently diverged 2026-08-14 (275 local-only vs 674 remote-only commits; bridge + unattended-reboot stranded), again 2026-08-30 (unpushed sibling crates broke the MacBook build), and 2026-09-03.

**Root cause of the third:** the publish flow COMMITS (`dev: android-arm64 vX published; next line vY`), so the publishing box always holds a commit nobody else has. Whoever pushes second rebases: `git pull --rebase`, then push.

**Tools:** `scripts/lib/sibling-push-gate.sh` names any path-dep repo with unpushed or uncommitted work (warn in dev.sh, refuses under `SEAM_STRICT=1` in deploy). `scripts/pull-deps.sh` fast-forwards photon's path-dep closure and prints the fix on divergence.

**Paths:** Leviathan wipes `$HOME` daily — `~/Code/photon` does not exist there. Scripts resolve their tree from `readlink -f "$0"` and print it.

**How to apply:**
- After a commit, check `git rev-parse HEAD` against `git ls-remote origin main`; "committed" ≠ "pushed".
- Before assuming work shipped, confirm its files on origin/main (`git cat-file -e origin/main:path`).
- "Missing symbol in fgtw/fluor" usually means a stale sibling crate — fast-forward siblings first.
Related: [[macbook-trails-remote]], [[push-after-landing]].
