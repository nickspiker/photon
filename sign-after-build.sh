#!/bin/bash
# Auto-sign script for Photon Messenger
# Usage: ./sign-after-build.sh [debug|release] [target]

set -e

BUILD_TYPE="${1:-debug}"
TARGET="${2:-}"

if [ -n "$TARGET" ]; then
    BINARY_PATH="target/$TARGET/$BUILD_TYPE/photon-messenger"
    [ "$TARGET" = "x86_64-pc-windows-gnu" ] && BINARY_PATH="$BINARY_PATH.exe"
else
    BINARY_PATH="target/$BUILD_TYPE/photon-messenger"
fi

if [ ! -f "$BINARY_PATH" ]; then
    echo "Binary not found: $BINARY_PATH"
    exit 1
fi

echo "Signing $BINARY_PATH..."

# Build the signer if it doesn't exist (native target only)
if [ ! -f "target/release/photon-signature-signer" ]; then
    echo "Building signature signer (one-time setup)..."
    cargo build --release --bin photon-signature-signer
    echo ""
fi

# Sign the binary
target/release/photon-signature-signer "$BINARY_PATH"

echo "✓ Signed successfully"
