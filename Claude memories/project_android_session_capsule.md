---
name: project_android_session_capsule
description: "Android de-attest-on-restart fix — boot-locked session capsule (SPEC'd in docs/, not yet built); root cause = Samsung kills the sticky broadcast"
metadata: 
  node_type: memory
  type: project
  originSessionId: 71fe9349-599c-4d2c-9970-3a988ee9a08f
---

**Bug:** on Android the app de-attested on a plain app restart. Root cause: the only session persistence was a **sticky broadcast**, which **dies with the app on Samsung** (empirically tested 2026-07-06) and likely other aggressive-OEM process killers → restart finds nothing → `tohu::session()` None → drops to the attest screen. Blank avatar/orb were downstream (no attestation → no handle_proof → nothing keyed on identity_seed loads).

**Fix — spec'd in `docs/android-session-persistence.md`, NOT yet implemented:** seal the 96-byte session roots (`identity_seed ‖ vault_seed ‖ handle_proof`) in a VSF capsule, ChaCha20-Poly1305 (`kete::encrypt_bytes`/`decrypt_bytes`, 256-bit key / 128-bit Poly1305 tag) under a per-boot **wairua** = `ihi::spaghettify(boot_id)`. Write to multiple tiers, try each on launch, first that AEAD-opens wins, all fail → attest → overwrite. Tiers: `filesDir` + external shadow (restart, no permission) · sticky broadcast + SAF shared-media (reinstall, same boot). **Power is the boundary**: same boot → decrypts (seamless restart); reboot → fresh boot_id → undecryptable → re-type handle (~weekly, the "logout"). Exposed tiers (sticky/SAF) key on `spaghettify(boot_id ‖ device_secret)` + a `device_secret` MAC (anti-forgery/brute-force). Mental model: no password — the device is the credential, the handle is a public username.

**boot_id validated on-device (2026-07-06):** `/proc/sys/kernel/random/boot_id` is READABLE from `untrusted_app`, stable across uninstall/reinstall (same boot), fresh after reboot → tohu reads `/proc` directly (`set_wairua_override` kept as dormant fallback for ROMs that block it). Considered+rejected: cloud backup (boot-lock negates it), a separate XOR pad (redundant with the device_secret mix). No migration (pre-public → nuke-and-restart; capsule has a version byte for future changes only). SAF backup = post-attest explain-then-permission flow. Desktop unchanged (tmpfs already right).

**Build order:** (1) tohu capsule crypto + wairua + tests → (2) tohu local tiers (`set_session_dir`, repoint `session`/`set_session`/`clear_session`) → (3) photon attest-write + launch try-order → (4) sticky broadcast carries the sealed capsule → (5) SAF backup flow. See [[project_session_registers]] (the register model) and [[project_secret_memory_hygiene]] (power = the userspace boundary).

UPDATE 2026-07-25: root cause of the RECURRING 'not signed in' (friend-S, Samsung) = tohu::set_boot_secret_override existed but was NEVER wired, so on ROMs whose SELinux blocks /proc/sys/kernel/random/boot_id the wairua couldn't derive → session never persisted → re-attest on every app restart. FIXED @ c40a3f2: Kotlin derives the boot secret from Settings.Global.BOOT_COUNT (per-boot-stable, no permission) + pre-API-24 per-install-random prefs fallback, handed to Rust via nativeNetworkInit before session wiring. Session now survives app-restart-within-boot on every ROM; re-attest only on genuine reboot.
