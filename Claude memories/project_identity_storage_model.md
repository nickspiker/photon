---
name: project-identity-storage-model
description: "Photon device identity is deterministic from the device fingerprint, not stored — drives storage scoping decisions"
metadata: 
  node_type: memory
  type: project
  originSessionId: 1c600b8c-f21d-40e3-a3b4-a5b74564adbd
---

Device identity is a deterministic Ed25519 keypair derived from the OS device fingerprint (Android: `Build.FINGERPRINT` → JNI → BLAKE3 → Ed25519; Linux: `/etc/machine-id` via `get_machine_fingerprint`). The keypair is NEVER stored to disk. Same device → same keypair every time, across reinstalls.

**Why:** This is the load-bearing assumption behind Photon's storage scope. Uninstalling the app does NOT lose identity — reinstall on the same device regenerates the same pubkey, FGTW sees the same person, contacts still recognize them. What gets lost on uninstall is *recoverable* state: message history (recoverable from peers + future cross-device sync), contact metadata, conversation state.

**How to apply:**
- Vault (both rings of the dual-ring) goes in app-private storage only — never shared storage, never asks for a "save my data" permission upfront. Android: `filesDir` (primary) + `getExternalFilesDir(null)` (shadow, different mount for in-session torn-write / partition-flake resilience).
- The shadow ring's job is intra-session resilience, NOT cross-uninstall recovery. Both rings dying with the uninstall is fine — nothing irrecoverable lives in them.
- User-saved bulk content (attachment exports, call recordings the user explicitly opted to keep, avatar exports) is the only thing that needs persistent shared storage. Request the permission just-in-time at the moment the user initiates the save — never at first launch. Matches modern Android UX guidance and Photon's privacy framing.
- If anyone proposes "store the keypair to disk so we can support multi-device" — that's a different feature (device-sync, deferred per [[project_vault_roadmap]]), not a storage decision. Storing the keypair would create an exfiltration surface that doesn't exist today.
