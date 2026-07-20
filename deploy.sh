#!/bin/bash
set -e

source scripts/lib/github.sh
source scripts/lib/manifest.sh

# Version scheme (2026-07-16): major.minor.patch. THIS SCRIPT does the release increment: whatever the tree holds (X.Y.0 fresh, or X.Y.P after dev publishes), the release ships X.(Y+1).0 — minor bumped, patch zeroed (patch 0 is RESERVED for releases; dev publishes bump the patch ≥1 and reach clients via the dev manifest).
# Ordering discipline (same as the dev publishes): refuse a dirty tree, bump, COMMIT THE BUMP FIRST — so every built binary embeds the actual release commit (no "+dirty") and the signed manifest stamps the same HEAD. A failure anywhere rolls that one commit back (trap below), leaving the tree exactly as it started.
# The publish lock keeps a dev-*.sh from bumping the version mid-deploy (and vice versa) — the same race that mis-stamped a dev manifest row on 2026-07-16.
manifest_take_publish_lock
if [ -n "$(git status --porcelain)" ]; then
    echo "ERROR: working tree is dirty — a release stamps HEAD into every binary + the signed manifest."
    echo "       Commit (or stash) first."
    git status --short | head -20
    exit 1
fi
CURRENT_VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([0-9]+\.[0-9]+\.[0-9]+)".*/\1/')
MAJOR=$(echo "$CURRENT_VERSION" | cut -d. -f1)
SHIP_VERSION=$(( $(echo "$CURRENT_VERSION" | cut -d. -f2) + 1 ))   # the MINOR is the deploy counter / dozenal cue
FULL_VERSION="${MAJOR}.${SHIP_VERSION}.0"
sed -i -E "s/^version = \"[0-9]+\.[0-9]+\.[0-9]+\"/version = \"${FULL_VERSION}\"/" Cargo.toml
cargo update --workspace --quiet 2>/dev/null || true
git add Cargo.toml Cargo.lock
git commit -q -m "release: v${SHIP_VERSION} (${FULL_VERSION})"
# Any failure from here rolls the release commit back — the tree returns to its pre-deploy state and the next attempt re-bumps cleanly.
trap 'echo ""; echo "DEPLOY FAILED — rolling back the release commit."; git reset --hard HEAD~1' ERR
echo "Deploying version: $FULL_VERSION (was $CURRENT_VERSION)"


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

# ── Signed update manifest (docs/updates.md): one signed VSF, every platform row, so running clients see this release + one-click update to it. Built with the SAME release key as the binaries. ──
echo ""
echo "Building signed release manifest..."
cargo build --release --bin photon-manifest
MANIFEST_TOOL=target/release/photon-manifest
R2_URL="https://brobdingnagian.holdmyoscilloscope.com/$R2_PATH"
b3() { b3sum "$1" | cut -d' ' -f1; }
COMMIT=$(git rev-parse HEAD)
"$MANIFEST_TOOL" --channel release --out /tmp/manifest-release.vsf \
    --artefact Linux   x86_64 "$FULL_VERSION" "$COMMIT" "$R2_URL/photon-messenger-linux-x86_64-release"  "$(b3 target/release/photon-messenger)" "$(stat -c %s target/release/photon-messenger)" \
    --artefact Linux   arm64  "$FULL_VERSION" "$COMMIT" "$R2_URL/photon-messenger-linux-arm64-release"   "$(b3 target/aarch64-unknown-linux-gnu/release/photon-messenger)" "$(stat -c %s target/aarch64-unknown-linux-gnu/release/photon-messenger)" \
    --artefact Windows x86_64 "$FULL_VERSION" "$COMMIT" "$R2_URL/photon-messenger-windows-release.exe"   "$(b3 target/x86_64-pc-windows-gnu/release/photon-messenger.exe)" "$(stat -c %s target/x86_64-pc-windows-gnu/release/photon-messenger.exe)" \
    --artefact macOS   x86_64 "$FULL_VERSION" "$COMMIT" "$R2_URL/photon-messenger-macos-intel-release"   "$(b3 target/x86_64-apple-darwin/release/photon-messenger)" "$(stat -c %s target/x86_64-apple-darwin/release/photon-messenger)" \
    --artefact macOS   arm64  "$FULL_VERSION" "$COMMIT" "$R2_URL/photon-messenger-macos-arm64-release"   "$(b3 target/aarch64-apple-darwin/release/photon-messenger)" "$(stat -c %s target/aarch64-apple-darwin/release/photon-messenger)" \
    --artefact Android arm64  "$FULL_VERSION" "$COMMIT" "$R2_URL/photon-messenger-android-release.apk"   "$(b3 android/app/build/outputs/apk/release/app-release.apk)" "$(stat -c %s android/app/build/outputs/apk/release/app-release.apk)"
wrangler r2 object put "$R2_BUCKET/$R2_PATH/manifest-release.vsf" \
    --file /tmp/manifest-release.vsf --content-type application/octet-stream --remote
echo "Release manifest published."
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

# Everything succeeded — release v$SHIP_VERSION ($FULL_VERSION) is public and its commit (made up
# top, before the builds) is now permanent: disarm the rollback.
trap - ERR

# Ring the release notice: the worker broadcasts "release" over the WS hub (every RUNNING client
# polls the signed manifest now instead of at its 6-8h cadence) and fires the FCM `updates` topic
# (wakes dozed Android subscribers). Advisory only — what installs is still gated by the manifest
# signature + stamp window — so best-effort: a failed curl just leaves everyone on the slow cadence.
echo ""
echo "Sending release notice (hub + FCM topic)..."
curl -s "https://fgtw.org/admin/release-notice?auth=f6d46fc44bd35b1b7204640d8cade6b2d01ef5e6ba96261200bcb728003c2724" || echo "release notice failed (non-fatal)"

# OPEN THE DEV LINE (2026-07-17): the tree must never rest at X.Y.0 — patch 0 IS the release marker,
# so a local dev build compiled from a .0 tree would masquerade as the release ("already on latest
# release" on a dev build, observed live). The release artifacts above embedded .0; from this commit
# on, every build is ≥ .1, and the next dev publish SHIPS .1 (publish-current-then-bump — see
# scripts/lib/manifest.sh).
DEV_OPEN="${MAJOR}.${SHIP_VERSION}.1"
sed -i -E "s/^version = \"[0-9]+\.[0-9]+\.[0-9]+\"/version = \"${DEV_OPEN}\"/" Cargo.toml
cargo update --workspace --quiet 2>/dev/null || true
git add Cargo.toml Cargo.lock
git commit -q -m "dev line open: v${DEV_OPEN} (release v${SHIP_VERSION} shipped at .0)"
git push

echo ""
echo "Install with:"
echo "  curl -sSfL https://brobdingnagian.holdmyoscilloscope.com/$R2_PATH/install-release.sh | sh"
echo "  powershell -ExecutionPolicy Bypass -c \"irm https://brobdingnagian.holdmyoscilloscope.com/$R2_PATH/install-release.ps1 | iex\""
