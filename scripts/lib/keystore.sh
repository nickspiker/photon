# Sourced, not executed. Resolves the keys dir, copies google-services.json into the Android
# project, and exports the TOKEN APK-signing keystore + password for Gradle. The password comes
# from the GNOME keyring (secret-tool); first run, store it once with the printed command.
# Callers cd to the repo root before sourcing, so the relative paths below resolve.

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

cp "$KEYS_DIR/google-services.json" android/app/

# TOKEN is the stack-wide APK signing key (gates the sibling-trust check at runtime).
KEYSTORE_PATH="$KEYS_DIR/TOKEN.p12"
if [ ! -f "$KEYSTORE_PATH" ]; then
    echo "ERROR: Keystore not found at $KEYSTORE_PATH"
    exit 1
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
export TOKEN_KEY_ALIAS="TOKEN"
