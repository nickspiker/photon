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

# Source-freeze aware: when deploy.sh exported SNAP_DIR (its reflink snapshot), build from the frozen tree so a live edit mid-deploy can't tear this build.
# CARGO_TARGET_DIR was already pinned to the real ./target by deploy.sh, so the signer paths below are unchanged.
# Run standalone (SNAP_DIR unset) → builds the live tree exactly as before.
snap_cargo() { ( cd "${SNAP_DIR:-.}" && cargo "$@" ); }

# Native: x86_64-unknown-linux-gnu (this box).
echo "Building x86_64 release binary..."
snap_cargo build --release
echo "Signing x86_64 binary..."
./target/release/photon-signature-signer target/release/photon-messenger

# Install the native binary to ~/.local/bin so `photon-messenger` runs this release build — same destination as the user installer, no download. (Only the native x86_64 binary; the arm64 cross-build below can't run on this box.)
# Stage-then-rename (mv is atomic on the same filesystem): a running instance holds the old inode open, so a plain cp fails with "Text file busy", but swapping the directory entry leaves the live process alone and the NEXT launch picks up the new binary.

INSTALL_DIR="$HOME/.local/bin"
mkdir -p "$INSTALL_DIR"
install -m755 target/release/photon-messenger "$INSTALL_DIR/photon-messenger.new"
mv -f "$INSTALL_DIR/photon-messenger.new" "$INSTALL_DIR/photon-messenger"
echo "Installed to $INSTALL_DIR/photon-messenger"

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
    snap_cargo build --release --target aarch64-unknown-linux-gnu

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
