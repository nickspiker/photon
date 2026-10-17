---
name: project-log-sweep-eats-fresh
description: "2026-08-21 a fresh log submission vanished in the hourly sweep window (never reproduced, unconvicted); the sweep now ages by the key's eagle osc and logs per-object verdicts; wrangler OAuth lesson"
metadata:
  type: project
---

2026-08-21 a fresh 2.8 MB submission vanished during fgtw-bootstrap's :17 hourly retention sweep (rule: delete photon-logs/ older than 24 h); the instrumented rerun kept every bait object, so the eater was never convicted.
The sweep (`scheduled` in fgtw-bootstrap src/lib.rs) now ages each object from the eagle osc embedded in its key (`<osc>-dev<id>.vsf`), uses `uploaded()` only for timestampless keys, and logs a per-object verdict that persists via `[observability]`.

**Why:** the one non-deterministic suspect (JS Date interop via `Object::uploaded()`) is gone, and any future deletion names itself.
**How to apply:** if a submission vanishes again, read the sweep verdict lines in Workers Logs first. Never crash-loop wrangler commands — concurrent refresh-token rotations burned the OAuth grant (recovery: `wrangler login`). Pull logs to a file ([[reference_log_pull]]).
