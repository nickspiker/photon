#!/bin/sh
set -e

echo "Photon Messenger DEVELOPMENT Installer"
echo "======================================="
echo ""
echo "This is a DEVELOPMENT build with logging enabled."
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
        # macOS has separate binaries for Intel and Apple Silicon
        if [ "$ARCH" = "arm64" ] || [ "$ARCH" = "aarch64" ]; then
            PLATFORM="macos-arm64"
        else
            PLATFORM="macos-intel"
        fi
        BINARY_NAME="photon-messenger"
        INSTALL_DIR="$HOME/.local/bin"
        APP_DIR="$HOME/Applications"
        APP_NAME="Photon Messenger.app"
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

# Download binary (flat naming with -development suffix)
DOWNLOAD_URL="https://brobdingnagian.holdmyoscilloscope.com/photon/photon-messenger-$PLATFORM-development"
TMP_BINARY="/tmp/photon-messenger-$$"

echo "Downloading Photon Messenger (dev)..."
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

# Remove ALL extended attributes on macOS (quarantine, etc) - must happen BEFORE signature check
if [ "$OS" = "Darwin" ]; then
    xattr -c "$TMP_BINARY" 2>/dev/null || true
fi

# Run binary once to self-verify signature
# (Binary will verify Ed25519 signature on startup and exit if invalid)
echo "Verifying signature..."
if ! "$TMP_BINARY" verify >/dev/null 2>&1; then
    echo "Error: Binary signature verification failed."
    echo "The downloaded file may be corrupted or tampered with."
    rm -f "$TMP_BINARY"
    exit 1
fi

echo "Signature verified"
echo ""

# Install binary
echo "Installing to $INSTALL_DIR..."
mkdir -p "$INSTALL_DIR"
mv "$TMP_BINARY" "$INSTALL_DIR/$BINARY_NAME"

# Clear all extended attributes from installed binary (macOS)
if [ "$OS" = "Darwin" ]; then
    xattr -c "$INSTALL_DIR/$BINARY_NAME" 2>/dev/null || true
fi

echo "Binary installed"
echo ""

# Create macOS .app bundle for Finder/Dock/Launchpad
if [ "$OS" = "Darwin" ]; then
    echo "Creating macOS app bundle..."

    # Create bundle structure
    mkdir -p "$APP_DIR/$APP_NAME/Contents/MacOS"
    mkdir -p "$APP_DIR/$APP_NAME/Contents/Resources"

    # Copy binary into bundle
    cp "$INSTALL_DIR/$BINARY_NAME" "$APP_DIR/$APP_NAME/Contents/MacOS/$BINARY_NAME"
    chmod +x "$APP_DIR/$APP_NAME/Contents/MacOS/$BINARY_NAME"

    # Download icon and convert to icns (macOS has iconutil built-in)
    ICON_URL="https://brobdingnagian.holdmyoscilloscope.com/photon/icon-1024.png"
    TMP_ICON="/tmp/photon-icon-$$"
    mkdir -p "$TMP_ICON.iconset"

    if curl -sSfL "$ICON_URL" -o "$TMP_ICON.png" 2>/dev/null; then
        # Create iconset with multiple sizes (sips is built into macOS)
        sips -z 16 16     "$TMP_ICON.png" --out "$TMP_ICON.iconset/icon_16x16.png" 2>/dev/null || true
        sips -z 32 32     "$TMP_ICON.png" --out "$TMP_ICON.iconset/icon_16x16@2x.png" 2>/dev/null || true
        sips -z 32 32     "$TMP_ICON.png" --out "$TMP_ICON.iconset/icon_32x32.png" 2>/dev/null || true
        sips -z 64 64     "$TMP_ICON.png" --out "$TMP_ICON.iconset/icon_32x32@2x.png" 2>/dev/null || true
        sips -z 128 128   "$TMP_ICON.png" --out "$TMP_ICON.iconset/icon_128x128.png" 2>/dev/null || true
        sips -z 256 256   "$TMP_ICON.png" --out "$TMP_ICON.iconset/icon_128x128@2x.png" 2>/dev/null || true
        sips -z 256 256   "$TMP_ICON.png" --out "$TMP_ICON.iconset/icon_256x256.png" 2>/dev/null || true
        sips -z 512 512   "$TMP_ICON.png" --out "$TMP_ICON.iconset/icon_256x256@2x.png" 2>/dev/null || true
        sips -z 512 512   "$TMP_ICON.png" --out "$TMP_ICON.iconset/icon_512x512.png" 2>/dev/null || true
        sips -z 1024 1024 "$TMP_ICON.png" --out "$TMP_ICON.iconset/icon_512x512@2x.png" 2>/dev/null || true

        # Convert iconset to icns
        iconutil -c icns "$TMP_ICON.iconset" -o "$APP_DIR/$APP_NAME/Contents/Resources/AppIcon.icns" 2>/dev/null || true

        # Cleanup
        rm -rf "$TMP_ICON.png" "$TMP_ICON.iconset"
    fi

    # Create Info.plist
    cat > "$APP_DIR/$APP_NAME/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>photon-messenger</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>com.photon.messenger</string>
    <key>CFBundleName</key>
    <string>Photon Messenger</string>
    <key>CFBundleDisplayName</key>
    <string>Photon Messenger</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleVersion</key>
    <string>1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.13</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
</dict>
</plist>
PLIST

    # Remove quarantine from entire app bundle
    xattr -cr "$APP_DIR/$APP_NAME" 2>/dev/null || true

    echo "App bundle created at $APP_DIR/$APP_NAME"
    echo ""
fi

# Add to PATH - handle macOS bash vs zsh properly
# macOS uses .bash_profile for login shells (Terminal.app), not .bashrc
# Modern macOS defaults to zsh which uses .zshrc
SHELL_RC=""
PATH_UPDATED=""

case "$SHELL" in
    */zsh)
        SHELL_RC="$HOME/.zshrc"
        ;;
    */bash)
        if [ "$OS" = "Darwin" ]; then
            # macOS bash uses .bash_profile for Terminal.app login shells
            SHELL_RC="$HOME/.bash_profile"
        else
            SHELL_RC="$HOME/.bashrc"
        fi
        ;;
    */fish)
        SHELL_RC="$HOME/.config/fish/config.fish"
        ;;
esac

# Create the rc file if it doesn't exist (common on fresh macOS)
if [ -n "$SHELL_RC" ]; then
    if [ ! -f "$SHELL_RC" ]; then
        touch "$SHELL_RC"
    fi

    if ! grep -q "$INSTALL_DIR" "$SHELL_RC" 2>/dev/null; then
        echo "" >> "$SHELL_RC"
        echo "# Added by Photon Messenger installer" >> "$SHELL_RC"
        echo "export PATH=\"\$PATH:$INSTALL_DIR\"" >> "$SHELL_RC"
        PATH_UPDATED="$SHELL_RC"
        echo "Added to PATH in $SHELL_RC"
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
        curl -sSfL https://brobdingnagian.holdmyoscilloscope.com/photon/app.png -o "$ICON_PATH" 2>/dev/null || true
    elif command -v wget >/dev/null 2>&1; then
        wget -q https://brobdingnagian.holdmyoscilloscope.com/photon/app.png -O "$ICON_PATH" 2>/dev/null || true
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
Comment=Decentralized secure messaging with passless authentication (DEV)
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

    echo "Desktop shortcut created"
fi

echo ""
echo "=========================================="
echo "Photon Messenger (DEV) installed!"
echo "=========================================="
echo ""
echo "DEVELOPMENT BUILD - Logging enabled"

if [ "$OS" = "Darwin" ]; then
    # macOS-specific instructions
    echo "Logs at: ~/Library/Application Support/photon/photon.log"
    echo ""
    echo "You can now:"
    echo ""
    echo "  1. Open from Finder: ~/Applications/Photon Messenger"
    echo "     (or search 'Photon' in Spotlight/Launchpad)"
    echo ""
    echo "  2. Drag to Dock for quick access"
    echo ""
    echo "  3. Run from terminal: photon-messenger"
    if [ -n "$PATH_UPDATED" ]; then
        echo "     (restart terminal or: source $PATH_UPDATED)"
    fi
    echo ""
else
    # Linux instructions
    echo "Logs at: stdout (run from terminal to see)"
    echo ""
    echo "Run 'photon-messenger' to start."
    echo "Or find 'Photon Messenger' in your application menu."
    echo ""
    if [ -n "$PATH_UPDATED" ]; then
        echo "Note: You may need to restart your terminal"
        echo "      or run: source $PATH_UPDATED"
        echo ""
    fi
fi
