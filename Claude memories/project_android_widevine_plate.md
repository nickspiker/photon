---
name: project_android_widevine_plate
description: "2026-11-02 Android plate candidate: Widevine deviceUniqueId read locally (no Google in the loop); probe BUILT, logs blake3 prefixes; verify reinstall / per-app / factory reset on the Pixel 8 Pro"
metadata:
  node_type: memory
  type: project
  originSessionId: d6c9db89-ccef-475b-b38b-64f51976930f
  modified: 2026-11-02T19:05:55.687Z
---

Until PIPE, Android's plate comes from the DMV (Nick 2026-11-02: "the plate numbers are going to come from the DMV, not me"). ANDROID_ID is per signing key but dies on a factory reset. Widevine `MediaDrm.PROPERTY_DEVICE_UNIQUE_ID` is burned in at manufacture (Google-issued, which is fine), read locally from the TEE with no permission, no session and no provisioning round trip (Google-in-the-loop, which is the no-go). Nick's wanted shape: `hash(serial ‖ photon signing cert)`.

BUILT 2026-11-02 (tohu `device::widevine_id()` + `device::signing_cert()`, photon `widevine_probe()` in jni_android.rs, dev APK built, not yet deployed): logs `widevine probe: id <blake3 prefix> (<n> bytes), level, cdm, plate(id ‖ signing cert) <prefix>` or `unreadable (<exception>)`. Never logs the raw ID.

RESULT 2026-11-02, Pixel 8 Pro, airplane mode on, dev build installed over USB: `id 7907cc35 (32 bytes), level L1, cdm 19.5.0@BV1A.250226.001, plate 9f315c60`. The read needs no network. Factory-reset comparison: pending (Nick resetting the phone next); the plate must come back 9f315c60.

To verify on the Pixel 8 Pro (Nick switches it from ferros to Android): deploy with scripts/android/dev-adb.sh, airplane mode, open Photon, pull the log, grep `widevine probe`. Compare across reinstall, a second app with our signing key (per-app scoping question, docs gap 5), and a factory reset only if Nick chooses. If it holds: add as the Android plate beside the unchanged v0 device_secret (never a migration of v0 without a plan). It is a reported value; it does not stop a modified client, only PIPE does.

Related: [[feedback_device_root_never_salted]], [[project_security_audit]], tohu/docs/oracle-hardening.md.
