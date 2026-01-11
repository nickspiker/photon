#!/bin/bash
set -e

echo "Building debug binary..."
cargo build --features development,logging

echo ""
echo "Signing debug binary..."
./target/debug/photon-signature-signer target/debug/photon-messenger

cp ./target/debug/photon-messenger /mnt/Chiton/MEGA/Code/photon/photon-messenger
echo "Copied to /mnt/Chiton/MEGA/Code/photon/photon-messenger"
# ./target/debug/photon-messenger /mnt/Chiton/MEGA/Code/photon/photon-messenger