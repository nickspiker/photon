#!/bin/bash
set -e

echo "Building release binary..."
cargo build --release

echo ""
echo "Signing binary..."
./target/release/photon-signature-signer target/release/photon-messenger

echo ""
echo "✓ Release build complete with signature!"
echo "Binary: target/release/photon-messenger"
