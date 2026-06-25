#!/bin/bash
# Desktop release: build (--release) + Ed25519 sign + install to ~/.local/bin, for the host OS.
# (The version-bumping, cross-target, actually-ship build is deploy.sh — not this.)
set -e
cd "$(dirname "$0")/.."
source scripts/lib/sign.sh
source scripts/lib/desktop.sh
build_sign_install release
