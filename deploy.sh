#!/bin/bash
set -e

# Read and increment binary version number
VERSION_FILE="v"
if [ -f "$VERSION_FILE" ]; then
    FILE_SIZE=$(stat -c%s "$VERSION_FILE")
    if [ "$FILE_SIZE" -eq 1 ]; then
        # 8-bit version
        VERSION=$(od -An -tu1 "$VERSION_FILE" | tr -d ' ')
    else
        # 16-bit version (little-endian)
        VERSION=$(od -An -tu2 "$VERSION_FILE" | tr -d ' ')
    fi
else
    VERSION=0
    FILE_SIZE=1
fi

NEW_VERSION=$((VERSION + 1))

# Upgrade to 16-bit if needed
if [ "$NEW_VERSION" -gt 255 ] && [ "$FILE_SIZE" -eq 1 ]; then
    # Write as 16-bit little-endian
    printf "\\x$(printf '%02x' $((NEW_VERSION & 0xFF)))\\x$(printf '%02x' $((NEW_VERSION >> 8)))" > "$VERSION_FILE"
elif [ "$FILE_SIZE" -eq 2 ]; then
    # Write as 16-bit little-endian
    printf "\\x$(printf '%02x' $((NEW_VERSION & 0xFF)))\\x$(printf '%02x' $((NEW_VERSION >> 8)))" > "$VERSION_FILE"
else
    # Write as 8-bit
    printf "\\x$(printf '%02x' $NEW_VERSION)" > "$VERSION_FILE"
fi

echo "Deploy version: $NEW_VERSION"

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
DOZENAL_VERSION=$(dozenal_names $NEW_VERSION)
echo "Dozenal version: $DOZENAL_VERSION"

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

# R2 bucket for releases (flat structure with release type in filename)
R2_BUCKET="holdmyoscilloscope"
R2_PATH="photon"

# Get Windows SHA256 for install script
WINDOWS_SHA256=$(cat target/x86_64-pc-windows-gnu/release/photon-messenger.exe.sha256)

echo ""
echo "Uploading to R2 ($R2_BUCKET/$R2_PATH)..."

# Upload all release binaries to R2 (flat naming with -release suffix)
wrangler r2 object put "$R2_BUCKET/$R2_PATH/photon-messenger-linux-release" \
    --file target/release/photon-messenger --remote
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
    --file install-release.sh --content-type text/plain --remote
wrangler r2 object put "$R2_BUCKET/$R2_PATH/icon-1024.png" \
    --file assets/icon-1024.png --content-type image/png --remote
wrangler r2 object put "$R2_BUCKET/$R2_PATH/app.png" \
    --file assets/icon-256.png --content-type image/png --remote

# Patch and upload install-release.ps1 with correct hash
sed "s/\$expectedHash = \"[A-F0-9]*\"/\$expectedHash = \"$WINDOWS_SHA256\"/" install-release.ps1 > /tmp/install-release.ps1
wrangler r2 object put "$R2_BUCKET/$R2_PATH/install-release.ps1" \
    --file /tmp/install-release.ps1 --content-type text/plain --remote

echo ""
echo "Linux, Windows, Redox, macOS Intel, macOS ARM64, Android binaries deployed to R2"
echo "  Windows SHA256: $WINDOWS_SHA256"

# Update website version and date
WEBSITE_DIR="/mnt/Chiton/MEGA/holdmyoscilloscope/photon"
DEPLOY_DATE=$(date +%Y-%m-%d)
sed -i "s/Version: [^·]*· Updated: [^<]*/Version: $DOZENAL_VERSION · Updated: $DEPLOY_DATE/" "$WEBSITE_DIR/index.html"
echo "Updated website: Version $DOZENAL_VERSION, Date $DEPLOY_DATE"

# Deploy website to Cloudflare Pages
echo ""
echo "Deploying website..."
(cd /mnt/Chiton/MEGA/holdmyoscilloscope && ./deploy.sh)

echo ""
echo "Install with:"
echo "  curl -sSfL https://brobdingnagian.holdmyoscilloscope.com/$R2_PATH/install-release.sh | sh"
echo "  powershell -ExecutionPolicy Bypass -c \"irm https://brobdingnagian.holdmyoscilloscope.com/$R2_PATH/install-release.ps1 | iex\""
