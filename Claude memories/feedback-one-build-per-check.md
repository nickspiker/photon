---
name: feedback-one-build-per-check
description: "Never run dev.sh (or any build) twice to inspect different parts of its output — capture once, grep the capture"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 2b62e116-08df-4b87-8444-016a23486f1b
  modified: 2026-08-26T05:50:58.213Z
---

Caught by Nick 2026-08-26: the pattern `./scripts/dev.sh | grep error; ./scripts/dev.sh | tail -1` runs TWO full compile+sign+install+relaunch cycles to read ONE build's output — on a battery-powered MacBook.

**Why:** dev.sh is not idempotent-cheap: every invocation is a full cargo build, codesign, install, and app relaunch. Doubling it doubles heat, battery drain, and photon restarts (each restart also churns vault/network state).

**How to apply:** one invocation, output to the scratchpad, then grep/tail the capture: `./scripts/dev.sh > $S/build.log 2>&1; tail -1 $S/build.log; grep -E "^error" $S/build.log`. Same for any expensive command (deploys, test suites, log pulls).
