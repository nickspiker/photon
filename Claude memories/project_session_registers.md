---
name: project_session_registers
description: "tohu session store holds register-shaped roots {identity_seed, vault_seed, handle_proof}, never the handle string; vault/network roots kept SEPARATE on purpose"
metadata:
  type: project
---

The device session store (`tohu::session()` / `set_session()` / `clear_session()`, 96 bytes under `$XDG_RUNTIME_DIR/tohu/session`) holds three 256-bit registers, never the handle string.

`tohu::SessionIdentity { identity_seed, vault_seed, handle_proof }`:
- **identity_seed** = `ihi::handle_to_hash(handle)` = BLAKE3(VSF-x(handle)) — network / contacts / avatar root.
- **vault_seed** = `tohu::handle_seed(handle)` = BLAKE3(NFC(handle)) — local-vault root.
- **handle_proof** = spaghettify(identity_seed), public, cached so resume skips the ~1 s proof.

**Keep identity_seed and vault_seed SEPARATE (different pre-images) — a security requirement.** A handle_proof seen on the wire must have no derivation path to the vault key. Never unify them.

Resume: `HandleQuery::query_resume(SessionIdentity)` hands the registers straight in; the handle string exists only transiently at first attest.
Related: [[project_device_identity_model]], [[project_token_terminology]].
