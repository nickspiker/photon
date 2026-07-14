# Sourced, not executed. Build + deploy functions for the Android APK. Source `keystore.sh` and `android-env.sh` before calling these. `android_build` sets `$APK_PATH` for the deployers.
#
# Profiles: `dev`     = --release + the `logging` feature + fluor's amber debug theme (orange bg tint / hairline / title — a dev build is never mistaken for release).
#           `release` = --release, no logging, normal theme (shippable).
# (Android always uses the release profile — debug builds are too large/slow on device.)

android_build() {
    local profile="$1"
    local features=""
    [ "$profile" = "dev" ] && features="--features logging,fluor/amber"

    echo "Building Photon for Android (arm64) — $profile..."
    PHOTON_ALLOW_RELEASE=1 cargo build --release --lib --target aarch64-linux-android $features

    local so="target/aarch64-linux-android/release/libphoton_messenger.so"
    if [ ! -f "$so" ]; then
        echo "ERROR: build failed — $so not found"
        exit 1
    fi

    echo "Copying .so into the Android project..."
    mkdir -p android/app/src/main/jniLibs/arm64-v8a
    cp "$so" android/app/src/main/jniLibs/arm64-v8a/

    # photon-specific: google-services.json (Firebase). The shared keystore lib exports TOKEN_KEYS_DIR but no longer copies this itself (it's app-agnostic now); other apps sharing keystore.sh skip it.

    cp "$TOKEN_KEYS_DIR/google-services.json" android/app/

    echo "Building APK with Gradle..."
    ( cd android && ./gradlew assembleRelease --rerun-tasks )

    APK_PATH="android/app/build/outputs/apk/release/app-release.apk"
    echo "APK built: $APK_PATH"
}

# Install the built APK over USB ADB.
deploy_adb() {
    echo "Installing via ADB..."
    "$ANDROID_HOME/platform-tools/adb" install -r "$APK_PATH"
    echo "ADB install complete."
}

# Find the phone on the LAN (Termux SSH on port 8022) and install over scp + ssh `pm install`. Order: cached IP -> default gateway (hotspot mode) -> nmap scan of local /8,/16,/24 nets.
deploy_network() {
    local CACHE_FILE="$HOME/.fairphone_ip_cache"
    local IP=""

    echo "Finding phone (Termux SSH :8022)..."
    if [ -f "$CACHE_FILE" ]; then
        local CACHED_IP
        CACHED_IP=$(cat "$CACHE_FILE")
        echo "Trying cached IP: $CACHED_IP"
        if nc -z -w 1 "$CACHED_IP" 8022 2>/dev/null; then
            echo "Phone found at cached IP."
            IP="$CACHED_IP"
        else
            echo "Cached IP not responding, scanning..."
        fi
    fi

    if [ -z "$IP" ]; then
        local GATEWAY
        GATEWAY=$(ip route | grep default | awk '{print $3}')
        if [ -n "$GATEWAY" ] && nc -z -w 1 "$GATEWAY" 8022 2>/dev/null; then
            echo "Phone found at gateway (hotspot): $GATEWAY"
            IP="$GATEWAY"
            echo "$IP" > "$CACHE_FILE"
        else
            local NETWORKS NETWORK
            NETWORKS=$(ip route | grep -E "/(8|16|24)" | grep -v "default" | awk '{print $1}' | grep -E "^(10\.|172\.(1[6-9]|2[0-9]|3[01])\.|192\.168\.)")

            for NETWORK in $NETWORKS; do
                echo "Scanning network: $NETWORK"
                IP=$(nmap -p 8022 --open "$NETWORK" 2>/dev/null | grep "Nmap scan report" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' | head -1)
                if [ -n "$IP" ]; then
                    echo "Phone found at $IP"
                    echo "$IP" > "$CACHE_FILE"
                    break
                fi
            done
        fi
    fi

    if [ -z "$IP" ]; then
        echo "Could not find a phone with SSH (:8022) on any local network."
        echo "APK is ready at: $APK_PATH"
        exit 1
    fi

    local SSH_OPTS="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR"
    local PHONE="u0_a10222@$IP"

    echo "Copying APK to Termux home on $IP..."
    if scp -P 8022 -i ~/.ssh/fairphone_key $SSH_OPTS "$APK_PATH" "$PHONE:/data/data/com.termux/files/home/photon.apk" 2>/dev/null; then
        echo "Installing..."
        ssh -p 8022 -i ~/.ssh/fairphone_key $SSH_OPTS "$PHONE" "su -c 'cp /data/data/com.termux/files/home/photon.apk /data/local/tmp/ && pm install -r /data/local/tmp/photon.apk && rm /data/local/tmp/photon.apk /data/data/com.termux/files/home/photon.apk'; exit" 2>/dev/null
        echo "Deployed at $(date '+%Y-%m-%d %H:%M:%S')"
    else
        echo "Failed to copy APK."
        exit 1
    fi
}
