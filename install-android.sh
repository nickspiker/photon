#!/bin/bash
# Install an already-built Photon APK to a connected Android device over ADB.
#
# This does NOT build — run build-android.sh (release) or build-android-dev.sh (dev + logging) first,
# or let deploy.sh produce the release APK. Then push it to the phone with this, no rebuild.
#
# Usage:
#   ./install-android.sh                 # newest of the release / debug APKs
#   ./install-android.sh release         # force the release APK
#   ./install-android.sh debug           # force the debug APK
#   ./install-android.sh path/to.apk     # an explicit APK
set -e

PKG="com.photon.messenger"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
RELEASE_APK="$SCRIPT_DIR/android/app/build/outputs/apk/release/app-release.apk"
DEBUG_APK="$SCRIPT_DIR/android/app/build/outputs/apk/debug/app-debug.apk"

# --- Locate adb (PATH first, then the SDK platform-tools) ---
if command -v adb >/dev/null 2>&1; then
    ADB=adb
elif [ -x "${ANDROID_HOME:-$HOME/android-sdk}/platform-tools/adb" ]; then
    ADB="${ANDROID_HOME:-$HOME/android-sdk}/platform-tools/adb"
else
    echo "ERROR: adb not found (not on PATH and not at \$ANDROID_HOME/platform-tools/adb)."
    exit 1
fi

# --- Pick the APK ---
case "${1:-}" in
    release) APK="$RELEASE_APK" ;;
    debug)   APK="$DEBUG_APK" ;;
    "" )
        # Newest of whichever exist.
        APK=""
        for cand in "$RELEASE_APK" "$DEBUG_APK"; do
            [ -f "$cand" ] || continue
            if [ -z "$APK" ] || [ "$cand" -nt "$APK" ]; then APK="$cand"; fi
        done
        ;;
    *) APK="$1" ;;  # explicit path
esac

if [ -z "$APK" ] || [ ! -f "$APK" ]; then
    echo "ERROR: no APK to install (looked for $RELEASE_APK and $DEBUG_APK)."
    echo "       Build one first: ./build-android.sh   (or ./build-android-dev.sh for logging)"
    exit 1
fi

# --- Require exactly one connected device ---
DEVICES=$("$ADB" devices | awk 'NR>1 && $2=="device" {print $1}')
COUNT=$(printf '%s\n' "$DEVICES" | grep -c . || true)
if [ "$COUNT" -eq 0 ]; then
    echo "ERROR: no device connected (check the USB cable / 'Allow USB debugging' prompt, or 'adb devices')."
    exit 1
elif [ "$COUNT" -gt 1 ]; then
    echo "ERROR: more than one device connected — disconnect the others or target one with ANDROID_SERIAL:"
    printf '  %s\n' $DEVICES
    exit 1
fi

echo "Installing $(basename "$APK") → $DEVICES"

# --- Install; on a signing-key mismatch, offer to uninstall (wipes app data) and retry ---
OUT=$("$ADB" install -r "$APK" 2>&1) || true
echo "$OUT"
if echo "$OUT" | grep -q 'INSTALL_FAILED_UPDATE_INCOMPATIBLE\|signatures do not match'; then
    echo ""
    echo "The installed app was signed with a different key (e.g. an older Photon)."
    echo "Reinstalling means UNINSTALLING $PKG first — this ERASES its on-device data (vault, keys)."
    printf "Uninstall and reinstall? [y/N] "
    read -r ans
    case "$ans" in
        y|Y)
            "$ADB" uninstall "$PKG" || true
            "$ADB" install "$APK"
            ;;
        *)
            echo "Aborted — nothing changed on the device."
            exit 1
            ;;
    esac
fi

echo ""
echo "Done. Launch from the app drawer, or:"
echo "  $ADB shell monkey -p $PKG -c android.intent.category.LAUNCHER 1"
