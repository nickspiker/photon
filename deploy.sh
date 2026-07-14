#!/bin/bash
set -e

source scripts/lib/github.sh

# The Cargo.toml version IS the displayed (dozenal) version, and it's the version we're currently working on. This deploy SHIPS that current version as-is; only on FULL SUCCESS do we bump the patch to the next number and commit. So Cargo.toml is never touched until everything has succeeded (no half-deployed dirty tree), and the number only advances on a successful deploy — never on test or full builds, which just inherit whatever the tree currently says.
SHIP_VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"[0-9]+\.[0-9]+\.([0-9]+)".*/\1/')
echo "Deploying version: $SHIP_VERSION"


# Convert to dozenal names for display
dozenal_names() {
    local n=$1
    local digits=("Zil" "Zila" "Zilor" "Ter" "Tera" "Teror" "Lun" "Luna" "Lunor" "Stel" "Stela" "Stelor")
    if [ "$n" -eq 0 ]; then
        echo "Zil"
        return
    fi
    local result=""
    while [ "$n" -gt 0 ]; do
        local digit=$((n % 12))
        if [ -z "$result" ]; then
            result="${digits[$digit]}"
        else
            result="${digits[$digit]} $result"
        fi
        n=$((n / 12))
    done
    echo "$result"
}
DOZENAL_VERSION=$(dozenal_names $SHIP_VERSION)
echo "Dozenal version: $DOZENAL_VERSION"

# Allow release builds (bypasses build.rs safety check)
export PHOTON_ALLOW_RELEASE=1

# Build and sign Linux x86_64 (native)
./build-release.sh

# Build Linux ARM64 (cross-compile)
echo ""
echo "Building Linux ARM64 release..."
CFLAGS_aarch64_unknown_linux_gnu="--sysroot=/usr/aarch64-redhat-linux/sys-root/fc42" \
PKG_CONFIG_SYSROOT_DIR=/usr/aarch64-redhat-linux/sys-root/fc42 \
PKG_CONFIG_PATH=/usr/aarch64-redhat-linux/sys-root/fc42/usr/lib64/pkgconfig \
PKG_CONFIG_ALLOW_CROSS=1 \
cargo build --release --target aarch64-unknown-linux-gnu

echo ""
echo "Signing Linux ARM64 binary..."
./target/release/photon-signature-signer target/aarch64-unknown-linux-gnu/release/photon-messenger

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
CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/mnt/Octopus/Code/osxcross/target/bin/aarch64-apple-darwin-clang-wrapper \
cargo build --release --target aarch64-apple-darwin

echo ""
echo "Signing macOS ARM64 binary..."
./target/release/photon-signature-signer target/aarch64-apple-darwin/release/photon-messenger

# Build Android APK (build-only; this script does its own R2 upload below)
echo ""
echo "Building Android release..."
./scripts/android/build.sh

# R2 bucket for releases (flat structure with release type in filename)
R2_BUCKET="holdmyoscilloscope"
R2_PATH="photon"

# Get Windows SHA256 for install script
WINDOWS_SHA256=$(cat target/x86_64-pc-windows-gnu/release/photon-messenger.exe.sha256)

echo ""
echo "Uploading to R2 ($R2_BUCKET/$R2_PATH)..."

# Upload all release binaries to R2 (flat naming with -release suffix)
wrangler r2 object put "$R2_BUCKET/$R2_PATH/photon-messenger-linux-x86_64-release" \
    --file target/release/photon-messenger --remote
wrangler r2 object put "$R2_BUCKET/$R2_PATH/photon-messenger-linux-arm64-release" \
    --file target/aarch64-unknown-linux-gnu/release/photon-messenger --remote
wrangler r2 object put "$R2_BUCKET/$R2_PATH/photon-messenger-windows-release.exe" \
    --file target/x86_64-pc-windows-gnu/release/photon-messenger.exe --remote
wrangler r2 object put "$R2_BUCKET/$R2_PATH/photon-messenger-redox-release" \
    --file target/x86_64-unknown-redox/release/photon-messenger --remote
wrangler r2 object put "$R2_BUCKET/$R2_PATH/photon-messenger-macos-intel-release" \
    --file target/x86_64-apple-darwin/release/photon-messenger --remote
wrangler r2 object put "$R2_BUCKET/$R2_PATH/photon-messenger-macos-arm64-release" \
    --file target/aarch64-apple-darwin/release/photon-messenger --remote
wrangler r2 object put "$R2_BUCKET/$R2_PATH/photon-messenger-android-release.apk" \
    --file android/app/build/outputs/apk/release/app-release.apk \
    --content-type application/vnd.android.package-archive --remote

# Upload install scripts and assets
wrangler r2 object put "$R2_BUCKET/$R2_PATH/install-release.sh" \
    --file installers/install-release.sh --content-type text/plain --remote
wrangler r2 object put "$R2_BUCKET/$R2_PATH/icon-1024.png" \
    --file assets/icon-1024.png --content-type image/png --remote
wrangler r2 object put "$R2_BUCKET/$R2_PATH/app.png" \
    --file assets/icon-256.png --content-type image/png --remote

# Patch and upload install-release.ps1 with correct hash
sed "s/\$expectedHash = \"[A-F0-9]*\"/\$expectedHash = \"$WINDOWS_SHA256\"/" installers/install-release.ps1 > /tmp/install-release.ps1
wrangler r2 object put "$R2_BUCKET/$R2_PATH/install-release.ps1" \
    --file /tmp/install-release.ps1 --content-type text/plain --remote

echo ""
echo "Linux ARM64, Linux x86_64, Windows, Redox, macOS Intel, macOS ARM64, Android binaries deployed to R2"
echo "  Windows SHA256: $WINDOWS_SHA256"

# Mirror the identical signed artefacts to a GitHub Release `v<n>` (redundant fallback behind R2).
# Same bytes as R2 — never rebuild per-destination — so the Windows SHA256 patched above holds everywhere.
GH_TAG="v$SHIP_VERSION"
echo ""
echo "Mirroring release to GitHub ($GH_TAG)..."
ensure_release "$GH_TAG" false
publish_github "$GH_TAG" "photon-messenger-linux-x86_64-release"  target/release/photon-messenger
publish_github "$GH_TAG" "photon-messenger-linux-arm64-release"   target/aarch64-unknown-linux-gnu/release/photon-messenger
publish_github "$GH_TAG" "photon-messenger-windows-release.exe"   target/x86_64-pc-windows-gnu/release/photon-messenger.exe
publish_github "$GH_TAG" "photon-messenger-redox-release"         target/x86_64-unknown-redox/release/photon-messenger
publish_github "$GH_TAG" "photon-messenger-macos-intel-release"   target/x86_64-apple-darwin/release/photon-messenger
publish_github "$GH_TAG" "photon-messenger-macos-arm64-release"   target/aarch64-apple-darwin/release/photon-messenger
publish_github "$GH_TAG" "photon-messenger-android-release.apk"   android/app/build/outputs/apk/release/app-release.apk
# Binaries only — no installer scripts on GitHub. The README carries the GitHub-fallback install
# commands (they fetch these assets by name from the latest release), so the scripts aren't needed here.

# Update website version and date
WEBSITE_DIR="/mnt/Chiton/MEGA/holdmyoscilloscope/photon"
DEPLOY_DATE=$(date +%Y-%m-%d)
sed -i "s/Version: [^·]*· Updated: [^<]*/Version: $DOZENAL_VERSION · Updated: $DEPLOY_DATE/" "$WEBSITE_DIR/index.html"
echo "Updated website: Version $DOZENAL_VERSION, Date $DEPLOY_DATE"

# Deploy website to Cloudflare Pages
echo ""
echo "Deploying website..."
(cd /mnt/Chiton/MEGA/holdmyoscilloscope && ./deploy.sh)

# Everything succeeded — version SHIP_VERSION is now public. Advance Cargo.toml to the next working
# version and commit, so the tree is ready for the next cycle and `v<n>` marks the just-shipped release.
NEXT_VERSION=$((SHIP_VERSION + 1))
sed -i -E "s/^(version = \"[0-9]+\.[0-9]+\.)[0-9]+\"/\1${NEXT_VERSION}\"/" Cargo.toml
git add Cargo.toml Cargo.lock && git commit -m "v$SHIP_VERSION deployed; working version → $NEXT_VERSION" && git push

echo ""
echo "Install with:"
echo "  curl -sSfL https://brobdingnagian.holdmyoscilloscope.com/$R2_PATH/install-release.sh | sh"
echo "  powershell -ExecutionPolicy Bypass -c \"irm https://brobdingnagian.holdmyoscilloscope.com/$R2_PATH/install-release.ps1 | iex\""
