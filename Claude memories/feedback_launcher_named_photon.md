---
name: feedback_launcher_named_photon
description: The resilient launcher (verify-and-fallback shim) is the `photon` command — ~/.local/bin/photon, installers/photon.sh — never "photon-launch"
metadata:
  type: feedback
---

The launcher Nick asked for is called `photon`: the command people type. It shipped as `photon-launch` (2026-08-29) against what he said; renamed 2026-10-29 ("I said to call it photon, not photon-launcher").

**Why:** it IS the program from the user's side — `photon-messenger` is the signed binary it verifies and execs, `photon` is how you run Photon.
**How to apply:** shim source `installers/photon.sh`, installed at `~/.local/bin/photon` (dev.sh + install-release.sh, which also delete an old `photon-launch`), published as `photon.sh` on R2, the updater's re-exec target and the lifeline unit's ExecStart. Never reintroduce a `-launch` name; see docs/resilient-launch.md.
