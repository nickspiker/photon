#!/bin/bash
set -e

# Local macOS ARM build + sign for quick iteration testing.
# No cross-compilation, no Linux box, no osxcross, no Cloudflare.
#
# Signing key lookup order:
#   1. PHOTON_SIGNING_KEY env var (explicit path)
#   2. ~/MEGA/Code/keys/photon-signing-key
#   3. ~/Library/Application Support/photon/photon-signing-key

export PHOTON_ALLOW_RELEASE=1

# Override the osxcross linker from .cargo/config.toml — that's for cross-compiling
# from Linux. When building natively on macOS, just use the system clang.
export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=clang

echo "Building release binary (native macOS ARM)..."
cargo build --release --features debug-keys

echo ""
echo "Signing binary..."
./target/release/photon-signature-signer target/release/photon-messenger

echo ""
echo "✓ Build complete!"
echo "Binary: target/release/photon-messenger"
echo ""
echo "To run:"
echo "  ./target/release/photon-messenger"
