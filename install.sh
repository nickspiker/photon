#!/bin/sh
set -e

echo "Photon Messenger Installer"
echo "============================"
echo ""

# Detect OS and architecture
OS=$(uname -s)
ARCH=$(uname -m)

case "$OS" in
    Linux*)
        PLATFORM="linux"
        BINARY_NAME="photon-messenger"
        INSTALL_DIR="$HOME/.local/bin"
        ;;
    Darwin*)
        PLATFORM="macos"
        BINARY_NAME="photon-messenger"
        INSTALL_DIR="$HOME/.local/bin"
        ;;
    *)
        echo "Error: Unsupported operating system: $OS"
        exit 1
        ;;
esac

if [ "$ARCH" != "x86_64" ] && [ "$ARCH" != "aarch64" ] && [ "$ARCH" != "arm64" ]; then
    echo "Error: Unsupported architecture: $ARCH"
    echo "Photon currently supports x86_64 and ARM64 only."
    exit 1
fi

echo "Detected: $OS ($ARCH)"
echo ""

# Download binary
DOWNLOAD_URL="https://holdmyoscilloscope.com/photon/binaries/photon-messenger-$PLATFORM"
TMP_BINARY="/tmp/photon-messenger-$$"

echo "Downloading Photon Messenger..."
if command -v curl >/dev/null 2>&1; then
    curl -sSfL "$DOWNLOAD_URL" -o "$TMP_BINARY"
elif command -v wget >/dev/null 2>&1; then
    wget -q "$DOWNLOAD_URL" -O "$TMP_BINARY"
else
    echo "Error: Neither curl nor wget found. Please install one and try again."
    exit 1
fi

# Make it executable
chmod +x "$TMP_BINARY"

# Remove quarantine flag on macOS
if [ "$PLATFORM" = "macos" ]; then
    xattr -d com.apple.quarantine "$TMP_BINARY" 2>/dev/null || true
fi

# Run binary once to self-verify signature
# (Binary will verify Ed25519 signature on startup and exit if invalid)
echo "Verifying signature..."
if ! "$TMP_BINARY" --verify >/dev/null 2>&1; then
    echo "Error: Binary signature verification failed."
    echo "The downloaded file may be corrupted or tampered with."
    rm -f "$TMP_BINARY"
    exit 1
fi

echo "✓ Signature verified"
echo ""

# Install binary
echo "Installing to $INSTALL_DIR..."
mkdir -p "$INSTALL_DIR"
mv "$TMP_BINARY" "$INSTALL_DIR/$BINARY_NAME"

echo "✓ Binary installed"
echo ""

# Add to PATH if not already there
SHELL_RC=""
if [ -n "$BASH_VERSION" ]; then
    SHELL_RC="$HOME/.bashrc"
elif [ -n "$ZSH_VERSION" ]; then
    SHELL_RC="$HOME/.zshrc"
else
    # Try to detect shell from $SHELL
    case "$SHELL" in
        */bash) SHELL_RC="$HOME/.bashrc" ;;
        */zsh) SHELL_RC="$HOME/.zshrc" ;;
        */fish) SHELL_RC="$HOME/.config/fish/config.fish" ;;
    esac
fi

if [ -n "$SHELL_RC" ] && [ -f "$SHELL_RC" ]; then
    if ! grep -q "$INSTALL_DIR" "$SHELL_RC" 2>/dev/null; then
        echo "" >> "$SHELL_RC"
        echo "# Added by Photon Messenger installer" >> "$SHELL_RC"
        echo "export PATH=\"\$PATH:$INSTALL_DIR\"" >> "$SHELL_RC"
        echo "✓ Added to PATH in $SHELL_RC"
    fi
fi

# Create desktop shortcut (Linux only)
if [ "$PLATFORM" = "linux" ]; then
    echo "Creating desktop shortcut..."

    # Download icon
    ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
    mkdir -p "$ICON_DIR"
    ICON_PATH="$ICON_DIR/photon-messenger.png"

    if command -v curl >/dev/null 2>&1; then
        curl -sSfL https://holdmyoscilloscope.com/photon/app.png -o "$ICON_PATH" 2>/dev/null || true
    elif command -v wget >/dev/null 2>&1; then
        wget -q https://holdmyoscilloscope.com/photon/app.png -O "$ICON_PATH" 2>/dev/null || true
    fi

    # Create .desktop file
    DESKTOP_DIR="$HOME/.local/share/applications"
    mkdir -p "$DESKTOP_DIR"
    DESKTOP_FILE="$DESKTOP_DIR/photon-messenger.desktop"

    cat > "$DESKTOP_FILE" <<EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=Photon Messenger
Comment=Decentralized secure messaging with passless authentication
Exec=$INSTALL_DIR/$BINARY_NAME
Icon=photon-messenger
Terminal=false
Categories=Network;InstantMessaging;
Keywords=messenger;chat;encryption;p2p;
StartupWMClass=photon-messenger
EOF

    chmod +x "$DESKTOP_FILE"

    # Update desktop database
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
    fi

    # Update icon cache
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
    fi

    echo "✓ Desktop shortcut created"
fi

echo ""
echo "=========================================="
echo "✓ Photon Messenger installed successfully!"
echo "=========================================="
echo ""
echo "Run 'photon-messenger' to start."
if [ "$PLATFORM" = "linux" ]; then
    echo "Or find 'Photon Messenger' in your application menu."
fi
echo ""
echo "Note: You may need to restart your terminal"
echo "      or run: export PATH=\"\$PATH:$INSTALL_DIR\""
echo ""
