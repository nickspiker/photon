---
name: project_unattended_reboot
description: "unattended auto-attest-on-reboot SHIPPED 2026-07-27 — off-by-default Security toggle, device-bound reboot capsule; re-typing the handle required BOTH to arm and disarm"
metadata:
  type: project
---

SHIPPED 2026-07-27 (tohu 8d66b19 + photon 23e13f5). The tohu session dies on reboot by design (wairua = per-boot key). Unattended mode deliberately defeats that for remote boxes the operator physically controls: `tohu::{store,load,clear}_reboot_capsule` seals the SessionIdentity under `device::device_secret()` (stable hardware fingerprint, not the wairua), so the same hardware re-derives the session with no handle typed; a copy elsewhere fails AEAD.

**Why:** Nick's passless remote box must come back after a power blip; it defeats the passless-identity point, hence off by default + red disclaimer.

**How to apply:** resume loads the capsule only if the normal session is gone; `set_unattended(on)` writes the `<config>/unattended_reboot` marker, forces autostart/background on and refreshes the capsule (off = deletes it); `refresh_reboot_capsule()` runs on every successful attest.
THE SECURITY GATE: arming AND disarming both require re-typing the handle in a confirm modal, verified against the live identity_seed.
Related: [[project_session_registers]], [[project_android_session_capsule]].
