---
name: feedback-pgrep-self-match
description: pgrep -f / pkill -f with a pattern that appears in your own command line matches (and STOPs or kills) your own shell — use exact pids, ps|grep "[x]yz", or kill -0 on a pid
metadata:
  type: feedback
---

2026-10-17, finishing the v121 deploy: `kill -STOP $(pgrep -f "rclone .*mahere")` stopped the tool's own bash (its command line contained the pattern), hanging the call; a watcher looping on `pgrep -f finish-v121.sh` would never have exited (its parent shell's cmdline held the string); `kill $(pgrep -f resume-rclone.sh)` killed the shell running it.

**Why:** the harness runs each command as `bash -c '<the whole command>'`, so any literal pattern in the command is in a live process's argv.
**How to apply:** pause/kill by exact pid (read once from `ps -eo pid,args | grep "[r]clone"` — the bracket keeps grep from matching itself), and wait on a process with `kill -0 <pid>`, never `pgrep -f <name>` in the same command line.
