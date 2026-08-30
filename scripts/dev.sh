#!/bin/bash
# Desktop dev: build (--features development) + Ed25519 sign + install to ~/.local/bin, for the host OS.
set -e
cd "$(dirname "$0")/.."
source scripts/lib/sign.sh
source scripts/lib/desktop.sh
# Merge-back guard: warn (never block) if any worktree holds work not on main — so an agent's isolated worktree can't silently rot and get redone. See scripts/lib/worktree-check.sh.
source scripts/lib/worktree-check.sh
worktree_check
# Seam tripwire rides preflight_gates (inside build_sign_install), warn-only there unless SEAM_STRICT=1 — no second call here (it double-printed).
build_sign_install dev
# Reload: nuke the running instance and launch the build that just landed — set -e means we only get here on a completed build+sign+install, so a broken build can never kill a working photon.
reload_photon
echo "completed $(date '+%F %T')"
