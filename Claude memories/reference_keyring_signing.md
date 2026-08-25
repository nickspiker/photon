---
name: reference_keyring_signing
description: Android signing password lives in Code/keys/TOKEN.p12.pass (not the OS keyring); the login keyring was re-keyed to EMPTY password to stop the autologin unlock-nag
metadata: 
  node_type: memory
  type: reference
  originSessionId: d83fbeaf-685c-4da4-8647-7b49de82fd2c
---

The GNOME login keyring was a silent build-killer and unusable remotely (fixed 2026-08-24/25):
- Leviathan uses lightdm AUTOLOGIN (autologin-user=nick in /etc/lightdm/lightdm.conf), and the lightdm-autologin PAM stack has NO pam_gnome_keyring — so no password is ever handed to the keyring at boot and it stays LOCKED every session. From a bridge/headless session the gcr prompter pops a GUI dialog nobody can answer, hanging any keyring caller. This killed a deploy silently at "Building Android release..." (keystore.sh's secret-tool lookup under set -e).

FIXES:
- TOKEN.p12's keystore password now lives in Code/keys/TOKEN.p12.pass (beside the .p12, synced to the private keys repo). scripts/lib/keystore.sh reads it FILE-FIRST; the OS keyring is only a guarded last-resort fallback. The Android build no longer touches the keyring at all. Threat model = physical access to Code/keys, which already holds raw ed25519 signing keys in the clear.
- The login keyring was re-keyed to an EMPTY password (contents preserved: Chrome/Chromium safe-storage, Proton, gh token, the two keystores). An empty-password login keyring auto-unlocks at session start with no prompt — the standard autologin fix. Backup of the old Zealand-encrypted keyring: ~/.local/share/keyrings/login.keyring.bak-prekey.
- The gnome-keyring v0 file format is offline-decryptable with the password: header gives iterations + 8-byte salt; KDF = SHA256(password‖salt) iterated `iterations` times → AES-128 key(16)‖iv(16); blob is AES-128-CBC; first 16 bytes of plaintext = MD5(rest) integrity check. Used this to lift the .p12 password without ever triggering the prompter.

OPEN: the gh GitHub token still lives in the keyring (works now that it auto-unlocks). Could move to Code/keys + GH_TOKEN for full keyring independence if wanted. Nick's login/keyring password is his universal one. See [[reference_backups]].
