#!/bin/bash
# Publish a Windows dev build (cross-compiled from Linux, logging on) to the R2 dev channel:
# build -> sign -> upload .exe + the PowerShell installer with the binary's SHA256 injected.
set -e
cd "$(dirname "$0")/../.."
source scripts/lib/sign.sh
source scripts/lib/publish.sh
source scripts/lib/github.sh
source scripts/lib/manifest.sh

# Refuse-dirty + patch-bump + commit BEFORE the build, so the binary embeds a clean HEAD whose commit is exactly what the signed manifest claims (docs/updates.md).
manifest_begin_dev_publish "windows-x86_64"

echo "Building Windows development binary..."
cargo build --target x86_64-pc-windows-gnu --features development
sign_binary debug x86_64-pc-windows-gnu

sha=$(cat target/x86_64-pc-windows-gnu/debug/photon-messenger.exe.sha256)

echo "Uploading to R2 (primary)..."
publish_r2 "photon-messenger-windows-development.exe" target/x86_64-pc-windows-gnu/debug/photon-messenger.exe
# Inject the freshly-built binary's hash into the PowerShell installer, then upload that copy.
sed "s/\$expectedHash = \"[A-F0-9]*\"/\$expectedHash = \"$sha\"/" installers/install-development.ps1 > /tmp/install-development.ps1
publish_r2 "install-development.ps1" /tmp/install-development.ps1 text/plain

echo "Publishing dev manifest row..."
manifest_publish_dev_row "Windows" "x86_64" "photon-messenger-windows-development.exe" target/x86_64-pc-windows-gnu/debug/photon-messenger.exe

echo "Mirroring to GitHub Releases (dev)..."
# Binary only — no installer script on GitHub. The GitHub fallback path is the README's copy-paste
# commands (they resolve the newest content-hashed asset via the API); a stale-cacheable installer
# script would defeat the point. The binary self-verifies on launch regardless of origin.
publish_github_dev "photon-messenger-windows-development.exe" target/x86_64-pc-windows-gnu/debug/photon-messenger.exe

echo ""
echo "Windows dev published (SHA256 $sha):"
echo "  $R2_BASE_URL/photon-messenger-windows-development.exe"
echo "  Install: powershell -ExecutionPolicy Bypass -c \"irm $R2_BASE_URL/install-development.ps1 | iex\""
