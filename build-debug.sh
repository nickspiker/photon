#!/bin/bash
set -e

echo "Building debug binary (signature verification skipped)..."
cargo build --features skip-sig,development
./target/debug/photon-messenger