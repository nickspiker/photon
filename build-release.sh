#!/bin/bash
# Multi-target Photon Messenger release build + sign.
#
# Builds the native (x86_64 Linux) target plus aarch64-unknown-linux-gnu
# via the cross toolchain configured in .cargo/config.toml. Each binary
# is signed in place with photon-signature-signer; the install scripts
# (install-release.sh) refer to these by `linux-x86_64` / `linux-arm64`
# suffix when uploaded to brobdingnagian.

set -e
cd "$(dirname "$0")"

export PHOTON_ALLOW_RELEASE=1

# Native: x86_64-unknown-linux-gnu (this box).
echo "Building x86_64 release binary..."
cargo build --release
echo "Signing x86_64 binary..."
./target/release/photon-signature-signer target/release/photon-messenger

# Cross: aarch64-unknown-linux-gnu.
# - .cargo/config.toml sets the linker + sysroot link flags
# - cc-rs needs CC_<target> + CFLAGS_<target> for C build deps
#   (blake3, ring, pqcrypto, etc.) since linker flags don't reach
#   the build.rs compile step
# - pkg-config needs the cross paths + ALLOW_CROSS so x11's build.rs
#   can probe the locally-staged x11.pc
echo
echo "Building aarch64 (arm64) release binary..."
CFLAGS_aarch64_unknown_linux_gnu="--sysroot=/usr/aarch64-redhat-linux/sys-root/fc42" \
CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
PKG_CONFIG_ALLOW_CROSS=1 \
PKG_CONFIG_PATH_aarch64_unknown_linux_gnu=/mnt/Octopus/Code/photon/cross-libs/aarch64/pkgconfig \
PKG_CONFIG_LIBDIR_aarch64_unknown_linux_gnu=/mnt/Octopus/Code/photon/cross-libs/aarch64/pkgconfig \
PKG_CONFIG_SYSROOT_DIR_aarch64_unknown_linux_gnu=/ \
    cargo build --release --target aarch64-unknown-linux-gnu

echo "Signing aarch64 binary..."
./target/release/photon-signature-signer target/aarch64-unknown-linux-gnu/release/photon-messenger

echo
echo "✓ Release builds complete with signatures!"
echo "Binaries:"
echo "  target/release/photon-messenger                              (x86_64)"
echo "  target/aarch64-unknown-linux-gnu/release/photon-messenger    (arm64)"
echo
echo "Upload to brobdingnagian as:"
echo "  photon-messenger-linux-x86_64-release"
echo "  photon-messenger-linux-arm64-release"
