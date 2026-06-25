#!/bin/bash
# Android dev (--release + logging) → install over the LAN (Termux SSH :8022). Logs: adb logcat -s photon
set -e
cd "$(dirname "$0")/../.."
source scripts/lib/keystore.sh
source scripts/lib/android-env.sh
source scripts/lib/android.sh
android_build dev
deploy_network
