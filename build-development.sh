#!/bin/bash
set -e

echo "Building debug binary..."
cargo build --features development

echo ""
echo "Signing debug binary..."
./target/debug/photon-signature-signer target/debug/photon-messenger

# Copy to MEGA sync folder for laptop testing
cp ./target/debug/photon-messenger /mnt/Chiton/MEGA/Code/photon/photon-messenger
echo "Copied to /mnt/Chiton/MEGA/Code/photon/photon-messenger"
