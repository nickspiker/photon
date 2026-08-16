---
name: project_session_registers
description: "tohu session store holds register-shaped roots {identity_seed, vault_seed, handle_proof}, never the handle string; vault/network roots kept SEPARATE on purpose"
metadata: 
  node_type: memory
  type: project
  originSessionId: cdf2afe5-967e-479c-85b5-0a67525ac5b4
---

The device session store (`tohu::session()` / `set_session()` / `clear_session()`, backed by `$XDG_RUNTIME_DIR/tohu/session`, 96 bytes) holds three 256-bit **registers**, never the handle string — the string is variable-length (no register / wairua slot) and Photon never displays the handle.

`tohu::SessionIdentity { identity_seed, vault_seed, handle_proof }`:
- **identity_seed** = `ihi::handle_to_hash(handle)` (`BLAKE3(VSF-x(handle))`) — the network / contacts / avatar root. `handle_proof = spaghettify(identity_seed)`.
- **vault_seed** = `tohu::handle_seed(handle)` (`BLAKE3(NFC(handle))`) — the local-vault root.
- **handle_proof** — public, cached so resume skips the ~1s `spaghettify`.

**Keep identity_seed and vault_seed SEPARATE (different pre-images) — this is a security requirement, not incidental.** A `handle_proof` seen on the wire must have no derivation path (not even one-way) to the vault key. Do NOT unify them to a single root.

Resume flow: `HandleQuery::query_resume(SessionIdentity)` (vs `query()` = first attest from a typed string). The worker resolves the three roots once on first attest, persists them, drops the string; resume hands them straight in → instant, string-free, no proof recompute. The handle string exists only transiently at first attest.

Plumbing: kete `FlatStorage::new_with_seed(vault_seed, …)`; avatar `*_from_seed` variants (identity_seed); `load_bootstrap_peers(…, identity_seed)`; photon_app holds `session: Option<tohu::SessionIdentity>`. The legacy `src/ui/app.rs` is orphaned (no `mod app`), so it was left unmigrated. Related: [[project_device_identity_model]], [[project_token_terminology]].
