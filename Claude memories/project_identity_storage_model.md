---
name: project-identity-storage-model
description: "Device identity is deterministic from the device fingerprint, never stored; the vault lives app-private only; shared storage only for user-initiated saves"
metadata:
  type: project
---

The device keypair derives from the OS device fingerprint (Android via JNI; Linux `/etc/machine-id` via `get_machine_fingerprint`) and is NEVER written to disk.
Same device → same key across reinstalls; a factory reset rotates it.

**Why:** uninstall loses only recoverable state (history comes back from the fleet and friends), and a stored keypair would be an exfiltration surface that doesn't exist today.
**How to apply:**
- The vault (both rings) lives in app-private storage only — Android `filesDir` primary + `getExternalFilesDir(null)` shadow (intra-session torn-write resilience, not cross-uninstall recovery).
- Never ask for a storage permission at launch; request it just-in-time when the user saves bulk content (attachment, wave or avatar exports).
- Never "store the keypair to support multi-device" — the fleet chain already does multi-device ([[project_keyring_design]]).
