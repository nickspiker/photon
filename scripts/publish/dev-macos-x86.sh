#!/bin/bash
# Publish a macOS x86_64 dev build (cross-compiled from Linux via osxcross, logging on) to the R2 dev channel: build -> sign -> upload binary.
# The osxcross clang-wrapper env mirrors deploy.sh's release recipe for the x86_64-apple-darwin target.
# Binary-only, like the other dev-* scripts; dev-linux.sh publishes the arch-agnostic install-development.sh.
set -e
cd "$(dirname "$0")/../.."
source scripts/lib/sign.sh
source scripts/lib/publish.sh
source scripts/lib/github.sh
source scripts/lib/manifest.sh

# Refuse-dirty + patch-bump + commit BEFORE the build, so the binary embeds a clean HEAD whose commit is exactly what the signed manifest claims (docs/updates.md).
manifest_begin_dev_publish "macos-x86_64"

echo "Building macOS x86_64 development binary..."
# The macOS SDK for crates whose build scripts ask `xcrun` where it is (coreaudio-sys behind cpal): no xcrun on Linux, so the osxcross SDK is named outright — the same line deploy.sh carries (2026-09-17: the arm64 dev publish failed on 'AudioUnit/AudioUnit.h' not found without it).
MACOS_SDK="$(ls -d /mnt/Harbor/Code/osxcross/target/SDK/MacOSX*.sdk | sort -V | tail -1)"
[ -d "$MACOS_SDK" ] || { echo "ERROR: no macOS SDK under osxcross/target/SDK"; exit 1; }
CC_x86_64_apple_darwin=/mnt/Harbor/Code/osxcross/target/bin/x86_64-apple-darwin-clang-wrapper \
COREAUDIO_SDK_PATH="$MACOS_SDK" \
CXX_x86_64_apple_darwin=/mnt/Harbor/Code/osxcross/target/bin/x86_64-apple-darwin-clang-wrapper \
OSXCROSS_TRIPLE=x86_64-apple-darwin \
CMAKE_TOOLCHAIN_FILE_x86_64_apple_darwin="$(pwd)/scripts/lib/osxcross-cmake.toolchain" \
    cargo build --features development --target x86_64-apple-darwin
sign_binary debug x86_64-apple-darwin

echo "Uploading to R2 (primary)..."
publish_r2 "photon-messenger-macos-x86_64-development" target/x86_64-apple-darwin/debug/photon-messenger

echo "Publishing dev manifest row..."
manifest_publish_dev_row "macOS" "x86_64" "photon-messenger-macos-x86_64-development" target/x86_64-apple-darwin/debug/photon-messenger

echo "Mirroring to GitHub Releases (dev)..."
publish_github_dev "photon-messenger-macos-x86_64-development" target/x86_64-apple-darwin/debug/photon-messenger

echo ""
echo "macOS x86_64 dev published:"
echo "  $R2_BASE_URL/photon-messenger-macos-x86_64-development"
echo "  Install: curl -sSfL $R2_BASE_URL/install-development.sh | sh"

# Publish landed — bump the patch + commit, opening the next dev line (publish-current-then-bump).
manifest_end_dev_publish
