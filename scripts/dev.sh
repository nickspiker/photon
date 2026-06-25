#!/bin/bash
# Desktop dev: build (--features development) + Ed25519 sign + install to ~/.local/bin, for the host OS.
set -e
cd "$(dirname "$0")/.."
source scripts/lib/sign.sh
source scripts/lib/desktop.sh
build_sign_install dev
