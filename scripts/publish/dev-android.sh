#!/bin/bash
# Publish an Android dev build (logging on) to the R2 dev channel: build the signed development APK -> upload.
# Mirror of dev-linux.sh for the Android target. Users download the APK straight from the site's Development section (no curl|sh installer — Android sideloads the .apk).
set -e
cd "$(dirname "$0")/../.."
source scripts/lib/keystore.sh
source scripts/lib/android-env.sh
source scripts/lib/android.sh
source scripts/lib/publish.sh
source scripts/lib/github.sh

echo "Building Android development APK (logging on)..."
android_build dev

echo "Uploading to R2 (primary)..."
publish_r2 "photon-messenger-android-development.apk" "$APK_PATH" application/vnd.android.package-archive

echo "Mirroring to GitHub Releases (dev)..."
publish_github_dev "photon-messenger-android-development.apk" "$APK_PATH"

echo ""
echo "Android dev published:"
echo "  $R2_BASE_URL/photon-messenger-android-development.apk"
