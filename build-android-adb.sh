#!/bin/bash
# Release build without logging (smaller binary, no logcat output)
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Find keys directory
if [ -d "/mnt/Octopus/Code/keys" ]; then
    KEYS_DIR="/mnt/Octopus/Code/keys"
elif [ -d "/mnt/Chiton/MEGA/Code/keys" ]; then
    KEYS_DIR="/mnt/Chiton/MEGA/Code/keys"
elif [ -d "$HOME/MEGA/code/keys" ]; then
    KEYS_DIR="$HOME/MEGA/code/keys"
else
    echo "ERROR: Cannot find keys directory"
    exit 1
fi

cp "$KEYS_DIR/google-services.json" "$SCRIPT_DIR/android/app/"

# Keystore config
KEYSTORE_PATH="$KEYS_DIR/nicks-apps.keystore"
if [ ! -f "$KEYSTORE_PATH" ]; then
    echo "ERROR: Keystore not found at $KEYSTORE_PATH"
    exit 1
fi
KEY_ALIAS="photon"

# Get password from GNOME Keyring (or prompt if not stored)
if [ -z "$PHOTON_KEYSTORE_PASSWORD" ]; then
    PHOTON_KEYSTORE_PASSWORD=$(secret-tool lookup service photon key keystore_password 2>/dev/null)
    if [ -z "$PHOTON_KEYSTORE_PASSWORD" ]; then
        echo "Password not in keyring. Run this once to store it:"
        echo "  secret-tool store --label='Photon Keystore' service photon key keystore_password"
        echo ""
        echo -n "Keystore password: "
        read -s PHOTON_KEYSTORE_PASSWORD
        echo ""
    fi
    export PHOTON_KEYSTORE_PASSWORD
fi

# Export for Gradle
export PHOTON_KEYSTORE_PATH="$KEYSTORE_PATH"
export PHOTON_KEY_ALIAS="$KEY_ALIAS"

# Set up Android NDK environment
export ANDROID_NDK_HOME=/home/nick/android-sdk/ndk/25.2.9519653
export ANDROID_HOME=/home/nick/android-sdk
NDK_BIN=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin
export PATH=$NDK_BIN:$PATH

# Create symlinks for ring crate (expects aarch64-linux-android-clang without API suffix)
if [ ! -f "$NDK_BIN/aarch64-linux-android-clang" ]; then
    ln -sf aarch64-linux-android21-clang "$NDK_BIN/aarch64-linux-android-clang"
    ln -sf aarch64-linux-android21-clang++ "$NDK_BIN/aarch64-linux-android-clang++"
fi

# Android target environment (ARM64 only)
export CC_aarch64_linux_android=$NDK_BIN/aarch64-linux-android21-clang
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$NDK_BIN/aarch64-linux-android21-clang

# Host build flags for build scripts (build.rs)
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="clang"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=mold"
export CC="clang"
export CXX="clang++"

echo "Building Photon for Android (arm64) - RELEASE (no logging)..."
PHOTON_ALLOW_RELEASE=1 cargo build --release --lib --target aarch64-linux-android

# Copy to Android project jniLibs
echo "Copying .so to Android project..."
mkdir -p android/app/src/main/jniLibs/arm64-v8a
cp target/aarch64-linux-android/release/libphoton_messenger.so android/app/src/main/jniLibs/arm64-v8a/

echo "Building APK with Gradle..."
cd android
./gradlew assembleRelease --rerun-tasks
cd ..

echo ""
echo "APK created at android/app/build/outputs/apk/release/"

echo "Installing via ADB..."
$ANDROID_HOME/platform-tools/adb install -r android/app/build/outputs/apk/release/app-release.apk

echo "RELEASE build complete! (no logging)"
