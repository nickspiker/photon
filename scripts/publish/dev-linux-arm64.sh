#!/bin/bash
# Publish a Linux ARM64 dev build (cross-compiled from x86_64, logging on) to the R2 dev channel: build -> sign -> upload binary.
# Companion to dev-linux.sh (which does the host arch only). The cross env mirrors build-release.sh's aarch64 recipe.
# No installer script here — dev-linux.sh already publishes install-development.sh (arch-agnostic), and Android/Mac follow the same binary-only pattern.
set -e
cd "$(dirname "$0")/../.."
source scripts/lib/sign.sh
source scripts/lib/publish.sh
source scripts/lib/github.sh
source scripts/lib/manifest.sh

# Refuse-dirty + patch-bump + commit BEFORE the build, so the binary embeds a clean HEAD whose commit is exactly what the signed manifest claims (docs/updates.md).
manifest_begin_dev_publish "linux-arm64"

echo "Building Linux ARM64 development binary..."
CFLAGS_aarch64_unknown_linux_gnu="--sysroot=/usr/aarch64-redhat-linux/sys-root/fc42" \
CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
PKG_CONFIG_ALLOW_CROSS=1 \
PKG_CONFIG_PATH_aarch64_unknown_linux_gnu=/mnt/Octopus/Code/photon/cross-libs/aarch64/pkgconfig \
PKG_CONFIG_LIBDIR_aarch64_unknown_linux_gnu=/mnt/Octopus/Code/photon/cross-libs/aarch64/pkgconfig \
PKG_CONFIG_SYSROOT_DIR_aarch64_unknown_linux_gnu=/ \
    cargo build --features development --target aarch64-unknown-linux-gnu
sign_binary debug aarch64-unknown-linux-gnu

echo "Uploading to R2 (primary)..."
publish_r2 "photon-messenger-linux-arm64-development" target/aarch64-unknown-linux-gnu/debug/photon-messenger

echo "Publishing dev manifest row..."
manifest_publish_dev_row "Linux" "arm64" "photon-messenger-linux-arm64-development" target/aarch64-unknown-linux-gnu/debug/photon-messenger

echo "Mirroring to GitHub Releases (dev)..."
publish_github_dev "photon-messenger-linux-arm64-development" target/aarch64-unknown-linux-gnu/debug/photon-messenger

echo ""
echo "Linux ARM64 dev published:"
echo "  $R2_BASE_URL/photon-messenger-linux-arm64-development"
echo "  Install: curl -sSfL $R2_BASE_URL/install-development.sh | sh"
