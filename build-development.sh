#!/bin/bash
set -e

echo "Building debug binary..."
cargo build --features development

echo ""
echo "Signing debug binary..."
./target/debug/photon-signature-signer target/debug/photon-messenger

# Install to ~/.local/bin so `photon-messenger` runs the build you just made — same destination as the user installer, no download.
INSTALL_DIR="$HOME/.local/bin"
mkdir -p "$INSTALL_DIR"
cp target/debug/photon-messenger "$INSTALL_DIR/photon-messenger"
echo "Installed to $INSTALL_DIR/photon-messenger"