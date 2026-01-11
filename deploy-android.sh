#!/bin/bash
set -eo pipefail  # Exit on error, including pipe failures

# Clean up any Android heap dump files
find . -name "*.hprof" -type f -delete 2>/dev/null || true

# Check Android SDK environment - try multiple common locations
if [ -d "/home/nick/android-sdk/build-tools" ]; then
    export ANDROID_HOME=/home/nick/android-sdk
elif [ -d "/home/nick/Android/Sdk/build-tools" ]; then
    export ANDROID_HOME=/home/nick/Android/Sdk
fi

# Find latest build tools version
LATEST_BUILD_TOOLS=$(ls "$ANDROID_HOME/build-tools/" 2>/dev/null | sort -V | tail -n 1)
if [ -z "$LATEST_BUILD_TOOLS" ]; then
    echo "Error: Could not find any build-tools in $ANDROID_HOME/build-tools/"
    exit 1
fi

APKSIGNER="$ANDROID_HOME/build-tools/$LATEST_BUILD_TOOLS/apksigner"

# Keystore config - try multiple locations
if [ -f "/mnt/Chiton/MEGA/Code/keys/nicks-apps.keystore" ]; then
    KEYSTORE_PATH="/mnt/Chiton/MEGA/Code/keys/nicks-apps.keystore"
elif [ -f "/home/nick/MEGA/code/keys/nicks-apps.keystore" ]; then
    KEYSTORE_PATH="/home/nick/MEGA/code/keys/nicks-apps.keystore"
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
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/25.2.9519653
export PATH=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH

# Android target environment (ARM64 only)
export CC_aarch64_linux_android=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android21-clang
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android21-clang

# Host build flags for build scripts (build.rs)
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="clang"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=mold"
export CC="clang"
export CXX="clang++"

echo "Building Photon for Android (arm64)..."
cargo build --features development --lib --target aarch64-linux-android

# Copy to Android project jniLibs
echo "Copying .so to Android project..."
mkdir -p android/app/src/main/jniLibs/arm64-v8a
SO_FILE="target/aarch64-linux-android/debug/libphoton_messenger.so"
if [ ! -f "$SO_FILE" ]; then
    echo "ERROR: Build failed - $SO_FILE not found"
    exit 1
fi
cp "$SO_FILE" android/app/src/main/jniLibs/arm64-v8a/

echo "Building APK with Gradle..."
cd android
./gradlew assembleRelease
cd ..

APK_PATH="android/app/build/outputs/apk/release/app-release.apk"

echo ""
echo "APK built: $APK_PATH"

# Cache file for storing the last known IP
CACHE_FILE="$HOME/.fairphone_ip_cache"

echo "Finding phone..."
IP=""

# First, try the cached IP if it exists
if [ -f "$CACHE_FILE" ]; then
    CACHED_IP=$(cat "$CACHE_FILE")
    echo "Trying cached IP: $CACHED_IP"
    if nc -z -w 1 "$CACHED_IP" 8022 2>/dev/null; then
        echo "Phone found at cached IP!"
        IP="$CACHED_IP"
    else
        echo "Cached IP not responding, scanning network..."
    fi
fi

# If no cached IP or it didn't work, try gateway first (for hotspot mode), then scan
if [ -z "$IP" ]; then
    # Check if phone is the gateway (hotspot mode)
    GATEWAY=$(ip route | grep default | awk '{print $3}')
    if [ ! -z "$GATEWAY" ] && nc -z -w 1 "$GATEWAY" 8022 2>/dev/null; then
        echo "Phone found at gateway: $GATEWAY"
        IP="$GATEWAY"
        echo "$IP" > "$CACHE_FILE"
    else
        # Scan networks
        NETWORKS=$(ip route | grep -E "/(8|16|24)" | grep -v "default" | awk '{print $1}' | grep -E "^(10\\.|172\\.(1[6-9]|2[0-9]|3[01])\\.|192\\.168\\.)")
        for NETWORK in $NETWORKS; do
            echo "Scanning network: $NETWORK"
            IP=$(nmap -p 8022 --open $NETWORK 2>/dev/null | grep "Nmap scan report" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' | head -1)
            if [ ! -z "$IP" ]; then
                echo "Phone found at $IP"
                echo "$IP" > "$CACHE_FILE"
                break
            fi
        done
    fi
fi

if [ -z "$IP" ]; then
    echo "Could not find phone with SSH running on any local network"
    echo "APK is ready at: $APK_PATH"
    exit 1
fi

echo "Using phone at $IP"
echo "Copying APK to Termux home..."
scp -P 8022 -i ~/.ssh/fairphone_key -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR "$APK_PATH" u0_a10222@$IP:/data/data/com.termux/files/home/photon.apk 2>/dev/null

if [ $? -eq 0 ]; then
    echo "Installing..."
    ssh -p 8022 -i ~/.ssh/fairphone_key -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR u0_a10222@$IP "su -c 'cp /data/data/com.termux/files/home/photon.apk /data/local/tmp/ && pm install -r /data/local/tmp/photon.apk && rm /data/local/tmp/photon.apk /data/data/com.termux/files/home/photon.apk'; exit" 2>/dev/null
    echo ""
    echo "Deployed at $(date '+%Y-%m-%d %H:%M:%S')"
else
    echo "Failed to copy APK."
    exit 1
fi
