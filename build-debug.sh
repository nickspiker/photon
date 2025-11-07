#!/bin/bash
set -e

echo "Building debug binary..."
cargo build

echo "Appending hash for self-verification..."
cargo run --bin hash-release -- target/debug/photon-messenger

echo ""
echo "✓ Debug build complete with hash verification!"
echo "Binary: target/debug/photon-messenger"
