#!/bin/bash
set -e

echo "Building debug binary..."
if [[ "$(uname)" == "Darwin" ]]; then
    export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=clang
fi
cargo build --features development

echo ""
echo "Signing debug binary..."
./target/debug/photon-signature-signer target/debug/photon-messenger

# cp ./target/debug/photon-messenger /mnt/Chiton/MEGA/Code/photon/photon-messenger
# echo "Copied to /mnt/Chiton/MEGA/Code/photon/photon-messenger"
# ./target/debug/photon-messenger /mnt/Chiton/MEGA/Code/photon/photon-messenger