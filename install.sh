#!/bin/sh
set -e

echo "Photon Messenger Installer"
echo "============================"
echo ""

# Check if cargo is installed
if ! command -v cargo >/dev/null 2>&1; then
    echo "Rust not found. Installing Rust toolchain..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

    # Source cargo env for current session
    . "$HOME/.cargo/env"

    echo ""
    echo "✓ Rust installed successfully!"
    echo ""
else
    echo "✓ Rust already installed"
    echo ""
fi

echo "Installing Photon Messenger..."
cargo install --locked photon-messenger

echo ""
echo "=========================================="
echo "✓ Photon Messenger installed successfully!"
echo "=========================================="
echo ""
echo "Run 'photon-messenger' to start."
echo ""
echo "Note: You may need to restart your terminal"
echo "      or run: source ~/.cargo/env"
