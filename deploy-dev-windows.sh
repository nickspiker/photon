#!/bin/bash
set -e

# Deploy Windows development build with logging to R2
# Fast debug builds (~30 sec) with --features development

R2_BUCKET="holdmyoscilloscope"
R2_PATH="photon"

echo "Building Windows development binary..."
cargo build --target x86_64-pc-windows-gnu --features development

echo ""
echo "Signing Windows binary..."
./sign-after-build.sh debug x86_64-pc-windows-gnu

# Get SHA256 for install script
WINDOWS_SHA256=$(cat target/x86_64-pc-windows-gnu/debug/photon-messenger.exe.sha256)

echo ""
echo "Uploading to R2..."
wrangler r2 object put "$R2_BUCKET/$R2_PATH/photon-messenger-windows-development.exe" \
    --file target/x86_64-pc-windows-gnu/debug/photon-messenger.exe --remote

# Generate and upload install-development.ps1 with correct hash
sed "s/\$expectedHash = \"[A-F0-9]*\"/\$expectedHash = \"$WINDOWS_SHA256\"/" install-development.ps1 > /tmp/install-development.ps1
wrangler r2 object put "$R2_BUCKET/$R2_PATH/install-development.ps1" \
    --file /tmp/install-development.ps1 --content-type text/plain --remote

echo ""
echo "Windows dev build deployed to R2"
echo "  Binary: https://brobdingnagian.holdmyoscilloscope.com/$R2_PATH/photon-messenger-windows-development.exe"
echo "  SHA256: $WINDOWS_SHA256"
echo ""
echo "Install with:"
echo "  powershell -ExecutionPolicy Bypass -c \"irm https://brobdingnagian.holdmyoscilloscope.com/$R2_PATH/install-development.ps1 | iex\""
