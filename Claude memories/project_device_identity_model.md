---
name: project_device_identity_model
description: "tohu device-identity model: per-platform oracle → device_secret, frozen v0 derivation, two honest axes (Security vs Recovery), attest seam for ferros/PIPE; deferred handle-salt collision hardening"
metadata:
  type: project
---

**tohu** (github.com/nickspiker/tohu) owns the per-platform device oracle and the frozen **v0** derivation (`handle_seed`, `vault_path_name`, `vault_anchor_key`).
Oracle: Linux `/etc/machine-id`, Windows `MachineGuid`, macOS `IOPlatformUUID`, Android `ANDROID_ID` read in-Rust via JNI (no Java change). `device_secret = BLAKE3(oracle)`.
ANDROID_ID changes on factory reset or a signing-key change → a NEW device identity (the 2026-08-29 ghost 1be949c1 class).

**Two orthogonal axes** (Ready-screen pips via `identity_posture()`): **Security** (theft/forgery resistance, bounded by root strength — every root today is readable = low) and **Recovery** (permanent-loss resistance — single-device Android = low). Never collapse them into one score.

**Attest seam:** `tohu::attest_with(domain, message, device_secret)` = `BLAKE3.derive_key("tohu-attest-<domain>-v0", message ‖ device_secret)`; tohu only appends the device secret. Photon's composite self-signing key (Ed25519 + PQ eggs) and its `app_signing_id` live in photon, not tohu. `attest` is the future ferros-syscall / PIPE seam (callers unchanged). Deferred: the project signing-key ceremony.

**Deferred hardening (low priority):** the FGTW device keypair is `derive(device_secret)`, handle-independent; a collision would be fatal but the 64-bit birthday bound is ~5 billion installs. If revisited, bind it with `handle_seed` too. Padding the oracle with model/make does NOT help (public facts, OTA drift).

**Sybil framing:** one owner per device; sign the whole stack (Photon, Lumis, basecalc) with ONE key so apps share `device_secret`. Casual-sybil-proof only; PIPE in silicon is the real root. Keystore/Play-Integrity rejected (Google dependency).
