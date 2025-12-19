#!/bin/bash
set -e

# Allow release builds (bypasses build.rs safety check)
export PHOTON_ALLOW_RELEASE=1

# Build and sign Linux
./build-release.sh

# Build Windows
echo ""
echo "Building Windows release..."
cargo build --release --target x86_64-pc-windows-gnu

echo ""
echo "Signing Windows binary..."
./target/release/photon-signature-signer target/x86_64-pc-windows-gnu/release/photon-messenger.exe

# Build Redox
echo ""
echo "Building Redox release..."
cargo build --release --target x86_64-unknown-redox

echo ""
echo "Signing Redox binary..."
./target/release/photon-signature-signer target/x86_64-unknown-redox/release/photon-messenger

# Build macOS Intel
echo ""
echo "Building macOS Intel release..."
CC_x86_64_apple_darwin=/mnt/Octopus/Code/osxcross/target/bin/x86_64-apple-darwin-clang-wrapper \
CXX_x86_64_apple_darwin=/mnt/Octopus/Code/osxcross/target/bin/x86_64-apple-darwin-clang-wrapper \
cargo build --release --target x86_64-apple-darwin

echo ""
echo "Signing macOS Intel binary..."
./target/release/photon-signature-signer target/x86_64-apple-darwin/release/photon-messenger

# Build macOS Apple Silicon
echo ""
echo "Building macOS ARM64 release..."
CC_aarch64_apple_darwin=/mnt/Octopus/Code/osxcross/target/bin/aarch64-apple-darwin-clang-wrapper \
CXX_aarch64_apple_darwin=/mnt/Octopus/Code/osxcross/target/bin/aarch64-apple-darwin-clang-wrapper \
cargo build --release --target aarch64-apple-darwin

echo ""
echo "Signing macOS ARM64 binary..."
./target/release/photon-signature-signer target/aarch64-apple-darwin/release/photon-messenger

# Build Android APK
echo ""
echo "Building Android release..."
./build-android.sh

# Copy to deployment folder
cp target/release/photon-messenger /mnt/Chiton/MEGA/holdmyoscilloscope/photon/photon-messenger-linux
cp target/x86_64-pc-windows-gnu/release/photon-messenger.exe /mnt/Chiton/MEGA/holdmyoscilloscope/photon/photon-messenger-windows.exe
cp target/x86_64-unknown-redox/release/photon-messenger /mnt/Chiton/MEGA/holdmyoscilloscope/photon/photon-messenger-redox
cp target/x86_64-apple-darwin/release/photon-messenger /mnt/Chiton/MEGA/holdmyoscilloscope/photon/photon-messenger-macos-intel
cp target/aarch64-apple-darwin/release/photon-messenger /mnt/Chiton/MEGA/holdmyoscilloscope/photon/photon-messenger-macos-arm64
cp android/app/build/outputs/apk/release/app-release.apk /mnt/Chiton/MEGA/holdmyoscilloscope/photon/photon-messenger.apk
cp install.sh /mnt/Chiton/MEGA/holdmyoscilloscope/photon/install.sh
cp assets/icon-1024.png /mnt/Chiton/MEGA/holdmyoscilloscope/photon/icon-1024.png

# Patch install.ps1 with the Windows binary SHA256 hash
WINDOWS_SHA256=$(cat target/x86_64-pc-windows-gnu/release/photon-messenger.exe.sha256)
sed "s/\$expectedHash = \"[A-F0-9]*\"/\$expectedHash = \"$WINDOWS_SHA256\"/" install.ps1 > /mnt/Chiton/MEGA/holdmyoscilloscope/photon/install.ps1

echo ""
echo "✓ Linux, Windows, Redox, macOS Intel, macOS ARM64, Android binaries, icon, and install scripts deployed"
echo "  Windows SHA256: $WINDOWS_SHA256"

wrangler pages deploy /mnt/Chiton/MEGA/holdmyoscilloscope --project-name=oscilloscope
