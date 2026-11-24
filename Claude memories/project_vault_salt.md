---
name: project_vault_salt
description: "Owner-approved 2026-11-24 kete format change: a chip-held vault salt in every identity-scope key, so shred/unbind = destroy the salt, never file deletes; NOT built yet"
metadata:
  node_type: memory
  type: project
  originSessionId: 17b90e8e-8b1a-448e-8ac7-22b057e9a0d7
  modified: 2026-11-24T19:55:33.640Z
---

**Decision (owner, 2026-11-24, relayed by ferros; "No? The TPM holds the salt", then "Yup"):** shred and unbind destroy keys, never files ("Sand on a beach my friend"; "if it does not get my consent from my other devices or a correct handle entry, nothing gets wiped").
- Identity-scope key = derive(addr ‖ vault_seed ‖ device_secret ‖ salt); today kete has no salt, so a keys-only shred is only a lock (handle + this hardware reopens everything).
- The salt lives in the security chip, never the disk:
  - TPM 2.0 (Windows, Linux): 32 random bytes in an NV index, read at attest; shred/unbind overwrite or undefine it.
  - Android StrongBox (rollback-resistant) and macOS Secure Enclave: no stored bytes come back, so salt = chip HMAC(non-exportable key, fixed label); shred/unbind delete the key.
  - No chip: the salt in the vault's device scope, accepting the flash leftover.
- The device root stays unsalted ([[feedback_device_root_never_salted]]); the salt is separate.
- Lands with CSTV-02's key root ([[project_vault_seed_stretch]]: kete `set_key_root`), with a dated conversion for existing vaults ([[feedback_pins_are_dated]]).
- No new on-disk location, no TRIM or hole punch.

**Why:** destruction must be one small key operation, and a salt on flash can survive in an unerased page; the chip's copy dies for real.

**How to apply:** plan kete + Photon after v157 and send the design to ferros for review. Until it lands, Photon's shred stays as it is (deletes files). Also queued from fgtw af8349a: drop the pre-attest Wipe (+ its clear_log), rotate the fleet key on FlowDone::Approved Ok, record the owner's three quotes in docs/settings-panel.md, keep the v0 contacts-backup blob sweep.
