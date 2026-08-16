---
name: feedback_build_dev_script
description: "Use ./scripts/dev.sh to compile/check photon, not bare cargo build"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: d79166a5-abe4-48c5-b486-214ed8068594
---

To compile-check + install a local photon dev build, run `./scripts/dev.sh` — do not run a bare `cargo build`.
(The old path `./build-development.sh` no longer exists; the script moved to `scripts/dev.sh`, which sources `scripts/lib/sign.sh` + `scripts/lib/desktop.sh` and runs `build_sign_install dev`.)

**Why:** bare `cargo build` thrashes the machine, and the script does build + check in one go — it compiles with `--features development`, surfaces all errors/warnings, then Ed25519-signs and installs the binary to `~/.local/bin/photon-messenger`.

**How to apply:** when validating photon changes compile, invoke `./scripts/dev.sh` and read its tail for errors/warnings. The release equivalent is `./build-release.sh`; full multi-platform deploy is `./deploy.sh`. For a fast errors-only check without the sign/install step, `cargo check --features development` also works. See [[feedback_commit_all]] for the commit convention.
