#!/bin/bash
# Publish a Windows dev build (cross-compiled from Linux, logging on) to the R2 dev channel:
# build -> sign -> upload .exe + the PowerShell installer with the binary's SHA256 injected.
set -e
cd "$(dirname "$0")/../.."
source scripts/lib/sign.sh
source scripts/lib/publish.sh

echo "Building Windows development binary..."
cargo build --target x86_64-pc-windows-gnu --features development
sign_binary debug x86_64-pc-windows-gnu

sha=$(cat target/x86_64-pc-windows-gnu/debug/photon-messenger.exe.sha256)

echo "Uploading to R2..."
publish_r2 "photon-messenger-windows-development.exe" target/x86_64-pc-windows-gnu/debug/photon-messenger.exe
# Inject the freshly-built binary's hash into the PowerShell installer, then upload that copy.
sed "s/\$expectedHash = \"[A-F0-9]*\"/\$expectedHash = \"$sha\"/" installers/install-development.ps1 > /tmp/install-development.ps1
publish_r2 "install-development.ps1" /tmp/install-development.ps1 text/plain

echo ""
echo "Windows dev published (SHA256 $sha):"
echo "  $R2_BASE_URL/photon-messenger-windows-development.exe"
echo "  Install: powershell -ExecutionPolicy Bypass -c \"irm $R2_BASE_URL/install-development.ps1 | iex\""
