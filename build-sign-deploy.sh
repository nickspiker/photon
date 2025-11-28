#!/bin/bash
set -e

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

# Build Android (arm64) - library only, no signing (APK signing handles that)
echo ""
echo "Building Android release..."
cargo build --release --lib --target aarch64-linux-android

# Copy to deployment folder
cp target/release/photon-messenger /mnt/Chiton/MEGA/holdmyoscilloscope/photon/photon-messenger-linux
cp target/x86_64-pc-windows-gnu/release/photon-messenger.exe /mnt/Chiton/MEGA/holdmyoscilloscope/photon/photon-messenger-windows.exe
cp target/x86_64-unknown-redox/release/photon-messenger /mnt/Chiton/MEGA/holdmyoscilloscope/photon/photon-messenger-redox

# Copy Android .so to Android project jniLibs (for APK building)
mkdir -p android/app/src/main/jniLibs/arm64-v8a
cp target/aarch64-linux-android/release/libphoton_messenger.so android/app/src/main/jniLibs/arm64-v8a/

echo ""
echo "✓ Linux, Windows, Redox, and Android binaries deployed"

wrangler pages deploy /mnt/Chiton/MEGA/holdmyoscilloscope --project-name=oscilloscope
