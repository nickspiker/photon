#!/bin/bash
# Build the signed release Android APK only — no install/deploy. Standalone, or invoked by deploy.sh.
# To build AND push to a device, use scripts/android/{dev,release}-{adb,network}.sh instead.
set -e
cd "$(dirname "$0")/../.."
source scripts/lib/keystore.sh
source scripts/lib/android-env.sh
source scripts/lib/android.sh
android_build release
echo "APK ready: $APK_PATH"
