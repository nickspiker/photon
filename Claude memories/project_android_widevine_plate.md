---
name: project_android_widevine_plate
description: "2026-11-02 Android ROOT = Widevine deviceUniqueId ‖ APK signing cert (verified identical thru a factory reset, read in airplane mode); legacy ANDROID_ID under a CONVERSION PIN until v168; vault/fleet conversion NOT built yet"
metadata:
  node_type: memory
  type: project
  originSessionId: d6c9db89-ccef-475b-b38b-64f51976930f
  modified: 2026-11-02T19:55:22.248Z
---

Until PIPE, Android's root comes from the DMV (Nick 2026-11-02: "the plate numbers are going to come from the DMV, not me"). Widevine `MediaDrm.PROPERTY_DEVICE_UNIQUE_ID` is burned in at manufacture (Google-issued, which is fine), read locally from the TEE with no permission, no DRM session and no provisioning round trip (Google-in-the-loop would be the no-go). Nick's shape: `hash(serial ‖ photon signing cert)`; the BLAKE3 is the unchanged v0 derivation, the fingerprint is the concatenation.

VERIFIED 2026-11-02 on the Pixel 8 Pro, airplane mode both times: `id 7907cc35 (32 bytes), level L1, cdm 19.5.0, plate 9f315c60` before AND after a factory reset, byte-identical. ANDROID_ID rotated with the wipe, so the phone became a new fleet device: Nick: "We just found the ira… if it rotates upon reset, that's not it."

BUILT 2026-11-02 (phase 1, v0.138.1 dev line): tohu `device::machine_fingerprint()` on Android = `widevine_root()` (ID ‖ cert) or legacy `android_id()` per `android_select_root()` (OnceLock, first call wins, required before any derivation; `android_root()` getter). kete `device_vault_present(app, secret)`. photon `select_android_root()` in jni_android.rs at the NetworkContext oracle site: new-root vault present or no vault → Widevine; only old-root vault present → legacy + "conversion owed" warn; Widevine unreadable → legacy (an error never flips a device onto a different root silently… it logs error). Posture: Android recovery pip 2 on the Widevine root. Logs carry blake3 prefixes only.

ANDROID ROOT CONVERSION PIN: delete at v168 (thirty versions after v138; grep "CONVERSION PIN" in tohu, kete, photon). Like [[project_old_self_import_removal]] (v145).

NOT BUILT (phase 2, the conversion): a phone on the legacy root must move to Widevine before v168. Two halves: (a) the device vault is NAMED from the secret and every entry address mixes it in, so there is no re-key; conversion = open old, load the app state through photon's own load paths, select new, save through the save paths (addresses cannot be enumerated into the new vault blindly). (b) the fleet device key changes: the old key (still readable until the wipe) binds the new key as a sibling (bind_device_held / claim-first ceremony on one phone) and then departs itself (depart_device_consented); friends and FGTW learn the new device thru the normal fleet paths. Identity succession (SuccessorRecord) is for whole-identity re-founding, not this.

Still open: per-app scoping of the Widevine ID (needs a second app signed with the TOKEN key; the cert fold-in makes the family one value regardless). Fresh-install note: the service, and so the oracle, only starts after the POST_NOTIFICATIONS dialog is answered; `adb shell pm grant com.photon.messenger android.permission.POST_NOTIFICATIONS` skips it for tests.

Related: [[feedback_device_root_never_salted]], [[project_security_audit]], [[project_device_identity_model]], tohu/docs/oracle-hardening.md.
