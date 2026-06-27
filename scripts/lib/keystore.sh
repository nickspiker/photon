# Sourced, not executed. Resolves the keys dir and exports the canonical TOKEN APK-signing keystore + password for the Android build (Gradle reads TOKEN_KEYSTORE_PATH / _PASSWORD / TOKEN_KEY_ALIAS; lumis's apksigner reads the same vars).
# The password comes from the GNOME keyring (secret-tool); first run, store it once with the printed command.
#
# This is the ONE keystore-resolution implementation shared across all of Nick's Android apps so they sign with the same key (alias 'token' in TOKEN.p12) and thus share a deterministic per-device ANDROID_ID — TOKEN auth across the whole app family.
# Keep it app-agnostic: anything app-specific (e.g. photon's google-services.json copy) belongs in the caller, not here.
#
# Callers cd to their repo root before sourcing.
# On failure this `return`s non-zero (it does NOT `exit`) so a sourcing script can decide how to handle it; check the return value.

if [ -d "/mnt/Octopus/Code/keys" ]; then
    KEYS_DIR="/mnt/Octopus/Code/keys"
elif [ -d "/mnt/Chiton/MEGA/Code/keys" ]; then
    KEYS_DIR="/mnt/Chiton/MEGA/Code/keys"
elif [ -d "$HOME/MEGA/code/keys" ]; then
    KEYS_DIR="$HOME/MEGA/code/keys"
else
    echo "ERROR: Cannot find keys directory"
    return 1 2>/dev/null || exit 1
fi

# TOKEN is the stack-wide APK signing key (gates the sibling-trust check + shared ANDROID_ID at runtime).
KEYSTORE_PATH="$KEYS_DIR/TOKEN.p12"
if [ ! -f "$KEYSTORE_PATH" ]; then
    echo "ERROR: Keystore not found at $KEYSTORE_PATH"
    return 1 2>/dev/null || exit 1
fi

if [ -z "$TOKEN_KEYSTORE_PASSWORD" ]; then
    TOKEN_KEYSTORE_PASSWORD=$(secret-tool lookup service token key keystore_password 2>/dev/null)
    if [ -z "$TOKEN_KEYSTORE_PASSWORD" ]; then
        echo "Password not in keyring. Run this once to store it:"
        echo "  secret-tool store --label='TOKEN Keystore' service token key keystore_password"
        echo ""
        echo -n "Keystore password: "
        read -s TOKEN_KEYSTORE_PASSWORD
        echo ""
    fi
fi
export TOKEN_KEYSTORE_PASSWORD
export TOKEN_KEYSTORE_PATH="$KEYSTORE_PATH"
export TOKEN_KEY_ALIAS="token"
export TOKEN_KEYS_DIR="$KEYS_DIR"
