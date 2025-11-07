#!/bin/bash
set -e

echo "Building release binary..."
cargo build --release

echo "Appending hash for self-verification..."
cargo run --release --bin hash-release -- target/release/photon-messenger

echo ""
echo "✓ Release build complete with hash verification!"
echo "Binary: target/release/photon-messenger"
