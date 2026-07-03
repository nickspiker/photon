#!/bin/bash
# Android dev (--release + logging) → install over USB ADB. Logs: adb pull /storage/emulated/0/Android/data/com.photon.messenger/files/photon.log.vsf, then photonlog (logcat is retired — everything lands in the VSF log)
set -e
cd "$(dirname "$0")/../.."
source scripts/lib/keystore.sh
source scripts/lib/android-env.sh
source scripts/lib/android.sh
android_build dev
deploy_adb
