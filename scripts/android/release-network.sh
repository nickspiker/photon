#!/bin/bash
# Android release (--release, no logging) → install over the LAN (Termux SSH :8022).
set -e
cd "$(dirname "$0")/../.."
source scripts/lib/keystore.sh
source scripts/lib/android-env.sh
source scripts/lib/android.sh
android_build release
deploy_network
