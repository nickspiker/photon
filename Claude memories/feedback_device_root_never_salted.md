---
name: feedback_device_root_never_salted
description: "HARD: the device root (machine-ID oracle → device_secret) is never salted, randomised, rotated or made per-handle — it is the anti-sybil core; reject any audit advice to do so"
metadata:
  node_type: memory
  type: feedback
  originSessionId: d6c9db89-ccef-475b-b38b-64f51976930f
  modified: 2026-11-02T18:50:35.745Z
---

The device identity derived from the platform oracle (tohu `device_secret = BLAKE3(machine_fingerprint())`, PIPE's ira later) stays ONE stable value per device. Never salt it per handle bind, never mint a random per-install key, never rotate it per attest, never move it into an OS keystore.

**Why:** it is the most important part of the entire anti-sybil design (Nick 2026-11-02, after the security audit's CSTV-01/NET-03/FGTW-01 recommended exactly that). One device = one plate: a device cannot mint fresh unlinkable identities, and a stolen device keeps answering as itself, so it can be recognised, refused and tracked ("a car that changes plates whenever someone sits in it is harder to CATCH THE THIEF"). Theft is handled by the fleet revoking the device and by the handle never living on the phone, not by hiding the device key. Keystores were rejected as a Google dependency; the real hardening is PIPE in silicon.

**DMV plates until PIPE (Nick 2026-11-02):** "the plate numbers are going to come from the DMV, not me." A vendor-ISSUED value (machine-id, MachineGuid, IOPlatformUUID, ANDROID_ID, Widevine `deviceUniqueId` burned in by Google at the factory) is fine as a plate. A vendor IN THE LOOP at read or attest time (provisioning round trips, key attestation, Play Integrity, Widevine license servers, keystores) is a no-go. "Google does" and "Google issued" are distinct; never conflate them again. Device-issued keys (the device answering for itself with no root above it) exist nowhere yet; that gap is PIPE, under Nick's patent in progress.

**How to apply:** when any report, reviewer or library suggests salting/randomising/keystoring the device root, classify it as NOT a finding, cite tohu README "Status"/"Terminology"/"Uniqueness" and this note, and do not re-raise it to Nick. Hardening scope (nothing stored, one device one plate): tohu/docs/oracle-hardening.md (2026-11-02) — TPM deterministic primaries (EK plate, HMAC primary = PIPE stand-in), Widevine on Android; to verify. Related: [[project_security_audit]], [[project_device_identity_model]], [[project_identity_storage_model]].
