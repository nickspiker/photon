---
name: reference_keyring_signing
description: Android signing password lives in Code/keys/TOKEN.p12.pass (not the OS keyring); the login keyring was re-keyed to EMPTY password to stop the autologin unlock-nag
metadata:
  type: reference
---

Fixed 2026-08-24/25: Leviathan uses lightdm AUTOLOGIN, whose PAM stack never unlocks gnome-keyring, so the keyring stayed locked and its GUI prompt hung headless callers — it silently killed a deploy at "Building Android release...".

- The TOKEN.p12 keystore password lives in `Code/keys/TOKEN.p12.pass` (synced to the private keys repo). `scripts/lib/keystore.sh` reads it FILE-FIRST; the OS keyring is only a guarded fallback. The Android build no longer touches the keyring.
- The login keyring was re-keyed to an EMPTY password, so it auto-unlocks at session start (the standard autologin fix); the gh token still lives there.
Related: [[reference_backups]].
