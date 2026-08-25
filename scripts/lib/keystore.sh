# Sourced, not executed. Resolves the keys dir and exports the canonical TOKEN APK-signing keystore + password for the Android build (Gradle reads TOKEN_KEYSTORE_PATH / _PASSWORD / TOKEN_KEY_ALIAS; lumis's apksigner reads the same vars). The password comes from the GNOME keyring (secret-tool); first run, store it once with the printed command.
#
# This is the ONE keystore-resolution implementation shared across all of Nick's Android apps so they sign with the same key (alias 'token' in TOKEN.p12) and thus share a deterministic per-device ANDROID_ID — TOKEN auth across the whole app family. Keep it app-agnostic: anything app-specific (e.g. photon's google-services.json copy) belongs in the caller, not here.
#
# Callers cd to their repo root before sourcing. On failure this `return`s non-zero (it does NOT `exit`) so a sourcing script can decide how to handle it; check the return value.

if [ -d "/mnt/Harbor/Code/keys" ]; then
    KEYS_DIR="/mnt/Harbor/Code/keys"
elif [ -d "/mnt/Chiton/MEGA/Code/keys" ]; then
    KEYS_DIR="/mnt/Chiton/MEGA/Code/keys"
elif [ -d "$HOME/MEGA/code/keys" ]; then
    KEYS_DIR="$HOME/MEGA/code/keys"
elif [ -d "$HOME/Code/keys" ]; then
    # The MacBook's MEGA sync lands here (2026-07-27) — same contents, no /mnt mounts on macOS.
    KEYS_DIR="$HOME/Code/keys"
else
    echo "ERROR: Cannot find keys directory (looked in /mnt/Harbor/Code/keys, /mnt/Chiton/MEGA/Code/keys, $HOME/MEGA/code/keys, $HOME/Code/keys)"
    return 1 2>/dev/null || exit 1
fi

# TOKEN is the stack-wide APK signing key (gates the sibling-trust check + shared ANDROID_ID at runtime).
KEYSTORE_PATH="$KEYS_DIR/TOKEN.p12"
if [ ! -f "$KEYSTORE_PATH" ]; then
    echo "ERROR: Keystore not found at $KEYSTORE_PATH"
    return 1 2>/dev/null || exit 1
fi

# Password source is per-OS: GNOME keyring via secret-tool on Linux, the macOS Keychain via security(1) on a Mac. Same service/account naming on both ('token' / 'keystore_password') so the store-once instructions below stay parallel.
# set -e TRAP: callers source this under `set -e`, and `VAR=$(cmd)` inherits cmd's exit status — so a failing secret-tool (LOCKED keyring, missing secret) aborts the whole source AT THE ASSIGNMENT, before the fallback below ever runs. That's a silent `exit 1` with zero output — a locked login-keyring killed a deploy at "Building Android release..." with no reason given (field 2026-08-24). `|| true` keeps the assignment succeeding so emptiness is handled deliberately, and a LOCKED keyring is named distinctly from a MISSING secret (they need different fixes).
if [ -z "$TOKEN_KEYSTORE_PASSWORD" ]; then
    if command -v secret-tool >/dev/null; then
        TOKEN_KEYSTORE_PASSWORD=$(secret-tool lookup service token key keystore_password 2>/dev/null) || true
        # Distinguish LOCKED (secret exists, keyring sealed) from ABSENT: `search` reports a locked object where `lookup` just returns empty.
        if [ -z "$TOKEN_KEYSTORE_PASSWORD" ] \
           && secret-tool search --all service token 2>&1 | grep -qi "locked"; then
            echo "The login keyring is LOCKED — the keystore password is stored but sealed." >&2
            echo "Unlock it (any ONE):" >&2
            echo "  • log into your GUI session (the login keyring auto-unlocks there), or" >&2
            echo "  • run in a graphical terminal: secret-tool lookup service token key keystore_password  (it will prompt to unlock), or" >&2
            echo "  • type the password at the prompt below to proceed this once." >&2
            echo "" >&2
        fi
    elif command -v security >/dev/null; then
        TOKEN_KEYSTORE_PASSWORD=$(security find-generic-password -s token -a keystore_password -w 2>/dev/null) || true
    fi
    if [ -z "$TOKEN_KEYSTORE_PASSWORD" ]; then
        echo "Password not in the OS keyring. Run this once to store it:"
        if command -v secret-tool >/dev/null; then
            echo "  secret-tool store --label='TOKEN Keystore' service token key keystore_password"
        else
            echo "  security add-generic-password -s token -a keystore_password -w"
        fi
        echo ""
        # A non-interactive caller (deploy piped/headless) has no TTY to read from — fail LOUDLY rather than export an empty password that gradle rejects 200 lines later with an inscrutable error.
        if [ ! -t 0 ]; then
            echo "ERROR: no keystore password and no TTY to prompt — aborting the signed build." >&2
            return 1 2>/dev/null || exit 1
        fi
        echo -n "Keystore password: "
        read -s TOKEN_KEYSTORE_PASSWORD
        echo ""
    fi
fi
if [ -z "$TOKEN_KEYSTORE_PASSWORD" ]; then
    echo "ERROR: keystore password is empty — refusing to build an unsigned/wrongly-signed APK." >&2
    return 1 2>/dev/null || exit 1
fi
export TOKEN_KEYSTORE_PASSWORD
export TOKEN_KEYSTORE_PATH="$KEYSTORE_PATH"
export TOKEN_KEY_ALIAS="token"
export TOKEN_KEYS_DIR="$KEYS_DIR"
