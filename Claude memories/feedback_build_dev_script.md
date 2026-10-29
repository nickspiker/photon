---
name: feedback_build_dev_script
description: "Use ./scripts/dev.sh to compile/check photon, not bare cargo build"
metadata:
  type: feedback
---

To compile-check + install a local photon dev build, run `./scripts/dev.sh` — never a bare `cargo build`.

**Why:** bare `cargo build` thrashes the machine; dev.sh builds with `--features development`, surfaces errors/warnings, Ed25519-signs and installs to `~/.local/bin/photon-messenger` in one go.

**How to apply:** run `./scripts/dev.sh` once, capture to the scratchpad, read the tail ([[feedback-one-build-per-check]]). Release = `./scripts/release/build-release.sh`; full deploy = `./scripts/release/deploy.sh`; Android = scripts/android/dev-adb.sh. Errors-only check without sign/install: `cargo check --features development`.
