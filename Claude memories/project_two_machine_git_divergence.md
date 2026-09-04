---
name: project_two_machine_git_divergence
description: Photon is developed on TWO machines (Leviathan desktop + a MacBook) whose git histories silently diverged; always verify local main == origin/main against the WIRE before assuming work shipped.
metadata: 
  node_type: memory
  type: project
  originSessionId: b192f764-fe07-4644-91fb-09156e2e7e05
  modified: 2026-08-30T21:56:00.412Z
---

Photon is developed on at least TWO machines: the **Leviathan desktop** (Linux, /mnt/Harbor/Code/photon) and a **MacBook**. Their git histories DIVERGED and neither box knew — this cost the user "the first week of my development in Europe as I thought that stuff was done" (2026-08-14).

**What happened (root-caused 2026-08-14):** Leviathan's local `main` forked from `origin/main` at commit 42615e0 (2026-07-17) and never reconciled. The MacBook kept pushing its line (per-device-lanes, up to 0.51.202); Leviathan kept committing the **bridge** feature + unattended-reboot locally and NEVER pushing them. Result: 275 local-only commits vs 674 remote-only commits. Of the 275, ~26 were REDUNDANT (the MacBook had done the same clutch/relay/history/attachments/chat work, often same commit subjects/hashes) — only **bridge + unattended-reboot (13 commits)** were genuinely stranded on the desktop.

**Fix applied 2026-08-14:** cherry-picked ONLY the 13 unique commits onto current origin/main (dropped the 26 redundant ones — proven already on remote), fast-forwarded main, pushed. bridge.rs now on origin/main @ e49b39b. Sibling crates fgtw (was 20 behind) + fluor (3 behind) also had to be fast-forwarded first or photon-main showed 66 phantom compile errors (missing fgtw::phonebook/scoped_blob, fluor::MultiTextbox). Safety tags left: pre-merge-leviathan-safety, remote-main-safety.

**2026-08-30 recurrence + structural fix:** it happened AGAIN — leviathan's agent left 14 unpushed commits across fgtw/fluor/kete/manifestus/nunc/tohu/vsf + uncommitted chirp (ring_from_hash) and rarangi; the MacBook could not build photon HEAD and every sibling pull lied "up to date". The discipline is now a GATE: `scripts/lib/sibling-push-gate.sh` (sibling_push_check) walks photon + every Cargo.toml path-dep and names any repo with unpushed commits OR uncommitted changes; warn-only in preflight_gates/dev.sh, refusing under `SEAM_STRICT=1` (deploy.sh). When a build prints SEAM lines, run the named push commands — and on the OTHER machine, pull all named siblings before trusting a compile.

**2026-09-03 — the third recurrence, and its ROOT CAUSE: the PUBLISH FLOW COMMITS.** A deploy on Leviathan mints `dev: android-arm64 v0.82.1 published; next line v0.82.2` locally, so the publishing box ALWAYS holds a commit no other machine has. That night the MacBook-side agent pushed ~8 commits while Leviathan sat on that one publish commit → textbook divergence with nobody at a keyboard ("how does that happen with nobody in the house?"). Not corruption, not a git bug: `git pull --rebase` then push, every time, on the box that published. Two agents on one repo means whoever pushes second rebases.

**PATHS: Leviathan wipes $HOME DAILY.** The checkout is `/mnt/Harbor/Code/photon` — `~/Code/photon` does NOT exist there and any instruction assuming it fails with "cannot change to '/home/nick/Code/photon'". Scripts must resolve their tree from their own `readlink -f "$0"` location, never from $HOME, and must PRINT the resolved paths so a wrong assumption is visible immediately.

New tool 2026-09-03: `scripts/pull-deps.sh` — the PULL-direction twin of sibling-push-gate.sh. Derives photon's Cargo.toml path-dep closure (12 repos, not the ~85 in the tree), fast-forwards each, prints resolved paths, and on divergence prints the local-only commits plus the exact fix command. Exit non-zero = not deploy-ready, so `./scripts/pull-deps.sh && ./scripts/release.sh` gates itself.

**How to apply — the standing discipline:**
- After ANY commit, verify `git rev-parse HEAD` == `git ls-remote origin main` (check the WIRE, not just `git status`). "Committed" != "pushed" != "on the machine that builds releases".
- Before assuming past work shipped, confirm the feature's FILES exist on origin/main (`git cat-file -e origin/main:path`), not just that a commit with that subject exists locally.
- The sibling crates (fgtw, fluor, tohu, vsf, spirix, manifestus) live in adjacent /mnt/Harbor/Code/ dirs and can independently fall behind their remotes — a photon that won't compile with "missing symbol in fgtw/fluor" usually means a stale sibling, not a code bug. Fast-forward the siblings first.
- deploy.sh silently failing (the Redox/set-e trap, fixed same day) MASKED this — a green-looking deploy that never pushed reinforced "it's done" when it wasn't. See the deploy.sh warning/failure-surfacing fix (commit e49b39b).

Relates to [[project_bridge]] (the stranded feature), [[per-device-lanes.md]] (the MacBook line that superseded the redundant desktop commits), [[push-after-landing.md]] (push discipline the memory repo already had but code didn't).

**2026-08-14 layout hazard:** photon_app.rs was split (commit 062612f) into a 3k root + 15 child modules under src/ui/photon_app/ (driver/render/protocol/status/devices/ceremony/conversation/messaging/sync/launch/settings/attachments/bridge/peers/input, each `impl PhotonApp` via `use super::*;`, private methods now pub(super)).
Any other-machine edits stranded against the old monolithic file will conflict wholesale — re-apply them into the matching child module by hand, don't merge-resolve the 26k-line deletion.
