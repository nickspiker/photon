#!/bin/bash
set -e

echo "Building debug binary (signature verification skipped)..."
cargo build --features skip-sig,development

# Copy to MEGA sync folder for laptop testing
cp ./target/debug/photon-messenger /mnt/Chiton/MEGA/Code/photon/photon-messenger
echo "Copied to /mnt/Chiton/MEGA/Code/photon/photon-messenger"
