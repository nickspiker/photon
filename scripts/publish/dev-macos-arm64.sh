#!/bin/bash
# Publish a macOS Apple Silicon dev build (cross-compiled from Linux via osxcross, logging on) to the R2 dev channel: build -> sign -> upload binary.
# The osxcross clang-wrapper env mirrors deploy.sh's release recipe for the aarch64-apple-darwin target.
# Binary-only, like the other dev-* scripts; dev-linux.sh publishes the arch-agnostic install-development.sh.
set -e
cd "$(dirname "$0")/../.."
source scripts/lib/sign.sh
source scripts/lib/publish.sh
source scripts/lib/github.sh
source scripts/lib/manifest.sh

# Refuse-dirty + patch-bump + commit BEFORE the build, so the binary embeds a clean HEAD whose commit is exactly what the signed manifest claims (docs/updates.md).
manifest_begin_dev_publish "macos-arm64"

echo "Building macOS ARM64 development binary..."
CC_aarch64_apple_darwin=/mnt/Octopus/Code/osxcross/target/bin/aarch64-apple-darwin-clang-wrapper \
CXX_aarch64_apple_darwin=/mnt/Octopus/Code/osxcross/target/bin/aarch64-apple-darwin-clang-wrapper \
CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/mnt/Octopus/Code/osxcross/target/bin/aarch64-apple-darwin-clang-wrapper \
    cargo build --features development --target aarch64-apple-darwin
sign_binary debug aarch64-apple-darwin

echo "Uploading to R2 (primary)..."
publish_r2 "photon-messenger-macos-arm64-development" target/aarch64-apple-darwin/debug/photon-messenger

echo "Publishing dev manifest row..."
manifest_publish_dev_row "macOS" "arm64" "photon-messenger-macos-arm64-development" target/aarch64-apple-darwin/debug/photon-messenger

echo "Mirroring to GitHub Releases (dev)..."
publish_github_dev "photon-messenger-macos-arm64-development" target/aarch64-apple-darwin/debug/photon-messenger

echo ""
echo "macOS ARM64 dev published:"
echo "  $R2_BASE_URL/photon-messenger-macos-arm64-development"
echo "  Install: curl -sSfL $R2_BASE_URL/install-development.sh | sh"
