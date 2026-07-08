#!/bin/bash
# Desktop dev: build (--features development) + Ed25519 sign + install to ~/.local/bin, for the host OS.
set -e
cd "$(dirname "$0")/.."
source scripts/lib/sign.sh
source scripts/lib/desktop.sh
# Merge-back guard: warn (never block) if any worktree holds work not on main — so an agent's
# isolated worktree can't silently rot and get redone. See scripts/lib/worktree-check.sh.
source scripts/lib/worktree-check.sh
worktree_check
build_sign_install dev
