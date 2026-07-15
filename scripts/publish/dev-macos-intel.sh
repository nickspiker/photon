#!/bin/bash
# Publish a macOS Intel dev build (cross-compiled from Linux via osxcross, logging on) to the R2 dev channel: build -> sign -> upload binary.
# The osxcross clang-wrapper env mirrors deploy.sh's release recipe for the x86_64-apple-darwin target.
# Binary-only, like the other dev-* scripts; dev-linux.sh publishes the arch-agnostic install-development.sh.
set -e
cd "$(dirname "$0")/../.."
source scripts/lib/sign.sh
source scripts/lib/publish.sh
source scripts/lib/github.sh
source scripts/lib/manifest.sh

# A dev publish bumps the PATCH so the shipped binary is newer + distinct (docs/updates.md). Before the build so build.rs bakes in the new patch + commit.
manifest_bump_dev_patch

echo "Building macOS Intel development binary..."
CC_x86_64_apple_darwin=/mnt/Octopus/Code/osxcross/target/bin/x86_64-apple-darwin-clang-wrapper \
CXX_x86_64_apple_darwin=/mnt/Octopus/Code/osxcross/target/bin/x86_64-apple-darwin-clang-wrapper \
    cargo build --features development --target x86_64-apple-darwin
sign_binary debug x86_64-apple-darwin

echo "Uploading to R2 (primary)..."
publish_r2 "photon-messenger-macos-intel-development" target/x86_64-apple-darwin/debug/photon-messenger

echo "Publishing dev manifest row..."
manifest_publish_dev_row "macos-intel" "photon-messenger-macos-intel-development" target/x86_64-apple-darwin/debug/photon-messenger
git add Cargo.toml Cargo.lock && git commit -q -m "dev: macos-intel $(manifest_full_version)" || true

echo "Mirroring to GitHub Releases (dev)..."
publish_github_dev "photon-messenger-macos-intel-development" target/x86_64-apple-darwin/debug/photon-messenger

echo ""
echo "macOS Intel dev published:"
echo "  $R2_BASE_URL/photon-messenger-macos-intel-development"
echo "  Install: curl -sSfL $R2_BASE_URL/install-development.sh | sh"
