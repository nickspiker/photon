#!/bin/bash
set -e

echo "Building debug binary..."
cargo build --features development

echo ""
echo "Signing debug binary..."
./target/debug/photon-signature-signer target/debug/photon-messenger

# Install to ~/.local/bin so `photon-messenger` runs the build you just made — same destination as the user installer, no download.
# Stage-then-rename (mv is atomic on the same filesystem): a running instance holds the old inode open, so a plain cp fails with "Text file busy", but swapping the directory entry leaves the live process alone and the NEXT launch picks up the new binary.
INSTALL_DIR="$HOME/.local/bin"
mkdir -p "$INSTALL_DIR"
install -m755 target/debug/photon-messenger "$INSTALL_DIR/photon-messenger.new"
mv -f "$INSTALL_DIR/photon-messenger.new" "$INSTALL_DIR/photon-messenger"
echo "Installed to $INSTALL_DIR/photon-messenger"