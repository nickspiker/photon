---
name: project_unattended_reboot
description: "unattended auto-attest-on-reboot SHIPPED (tohu 8d66b19 + photon 23e13f5) — off-by-default Security toggle, device-bound reboot capsule defeats the wairua's deliberate reboot-death, handle re-entry required BOTH to arm and disarm"
metadata: 
  node_type: memory
  type: project
  originSessionId: f7df2de2-8ae4-45a0-a572-2865a6e4ac5f
---

Auto-attest-on-reboot (unattended mode) SHIPPED 2026-07-27. The boot-locked tohu session dies on reboot BY DESIGN (wairua = per-boot key from boot_id → capsule undecryptable after reboot → typed-attest screen). Unattended mode deliberately defeats this for remote failsafe boxes the operator physically controls: `tohu::{store,load,clear}_reboot_capsule` seals the 96-byte SessionIdentity under `device::device_secret()` (BLAKE3 of the stable hardware fingerprint, NOT the wairua), so a reboot on the SAME hardware re-derives the session with no handle typed. Device-bound: a copy elsewhere fails to open (wrong fingerprint → AEAD auth fail). Magic TOHUREB1; tohu still depends on no kete (chacha20poly1305 direct).

**Why:** user has a passless remote box (rustdesk replacement context) that must come back up unattended after a power blip; the whole passless-identity point is defeated, hence off-by-default + big red disclaimer.

**How to apply:** photon resume (photon_app.rs, before `tohu::session()` check) loads the capsule iff normal session gone; `set_unattended(on)` writes `<config>/unattended_reboot` marker + forces autostart/background on (meaningless without relaunch-at-boot) and refreshes capsule, or shreds on off; `refresh_reboot_capsule()` fires on every successful attest. THE SECURITY GATE: arming AND disarming both require re-typing the handle in a confirm modal (verified vs live session identity_seed via `Handle::to_identity_seed`) — flipping a device-becomes-you switch from an already-unlocked screen must prove the operator. Modal reuses the attach-overlay card idiom (unattended_confirm: Option<bool> = target state). Security page grew 11→15 rows. Related: [[project_session_registers]], [[project_android_session_capsule]] (the boot-locked capsule this inverts), [[project_identity_storage_model]].
