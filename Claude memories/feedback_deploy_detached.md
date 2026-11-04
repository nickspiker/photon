---
name: feedback-deploy-detached
description: "HARD: never run deploy.sh (or any multi-platform publish) as a tool-backgrounded command — the shell tool caps at 10 min and would kill it mid-publish; launch it with setsid nohup into a log file and watch the log"
metadata:
  type: feedback
---

A full deploy (`scripts/release/deploy.sh`) runs 20+ minutes across eight platform builds and the uploads. The Bash tool's `run_in_background` still carries the tool timeout (max 600000 ms), so a deploy launched that way is killed at ten minutes, possibly inside the publish phase.

**Why:** 2026-11-03, the v140 deploy was started as a backgrounded tool command; caught in the lint phase (nothing public yet), stopped, and relaunched detached. A kill during the uploads would skip the ERR/EXIT traps (SIGKILL), leaving a local bump commit or a half-published folder the design otherwise prevents.

**How to apply:** `setsid nohup bash -c 'scripts/release/deploy.sh; echo "DEPLOY-EXIT $?"' > <scratch>/deploy-vN.log 2>&1 < /dev/null & disown`, then a Monitor (or a Bash until-loop) on the log for `DEPLOY-EXIT`, `ERROR`, `GATE:`, `SEAM:`, `DID NOT SHIP`, `shipped`. Before launching: no CLUTCH/WAVE live in the desktop log ([[feedback-reload-mid-ceremony]]), tree clean, every sibling pushed and clean (the strict seam gate refuses a dirty sibling — a lone `Cargo.lock` re-lock in a sibling is the usual cause; commit it as "lock: sibling path-dep re-lock"). Do not commit to photon main while the deploy runs: it pushes the bump and the advance itself.
