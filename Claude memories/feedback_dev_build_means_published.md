---
name: feedback-dev-build-means-published
description: "HARD (Nick 2026-11-04): 'dev build' = a PUBLISHED dev-channel build (scripts/release/publish/dev-<platform>.sh); a local compile is a 'compile check' or 'test', never a 'dev build'"
metadata:
  type: feedback
---

Saying "I'm running an Android dev build" tells Nick a build is going to the dev channel where a tester can install it. A local `android_build dev` or a cargo check is a compile check; call it that.

**Why:** 2026-11-04, Jeff's reinstall fix: I said "the Android dev build is compiling" meaning a local compile check, and Nick expected Jeff to be able to install it ("So how's Jeff getting this dev build?").
**How to apply:** "dev build" only when running (or about to run) `scripts/release/publish/dev-<platform>.sh` (or `scripts/dev.sh` on the desktop, which installs locally — say "desktop reload" for that). Otherwise "compile check" / "the test gate". A dev publish runs detached like a deploy ([[feedback-deploy-detached]]) and Nick's ask or an announced dev build is the go.
