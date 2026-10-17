---
name: project_storage_layering
description: "Storage layers: vault (kete) / chain state / rārangi conversation DB; vault keys = vault_key(domain, scope), never paths/hex/base64; attachments live outside the vault by design"
metadata:
  type: project
---

Three layers (migration started 2026-06-26, COMPLETE per the 2026-08-18 audit):
1. **Vault** = kete::FlatStorage, one `.vsf` per handle. The vault filename is the ONLY encoded name allowed on disk under `~/.config/photon/`.
2. **Chain state** = the crypto ratchet (`FriendshipChains`) — machinery, not content ("storage != chain state", Nick). A kete entry keyed by the conversation.
3. **Content** = rārangi rows (messages keyed by eagle_time per conversation, `src/storage/contacts.rs`).

**Canonical key rule (no paths, no hex, no base64):** `vault address = vault_key(domain_word, scope_bytes)` with scope = my vault_seed (self/global), their seed (per-peer), or friendship_id (per-conversation).
rārangi stays string-keyed because table/pk/seq are genuinely strings, never laundered binary.

Attachments are content-addressed sealed files OUTSIDE the vault by design: dual-ring doubles vault writes and multi-MB values trigger UI-freezing vault grows (`src/storage/mod.rs`).

**Why:** photon was written against a filesystem mental model; tree-shaped string keys laundered binary identity through text.
**How to apply:** any new stored thing gets a `vault_key(domain, scope)` address; never a path-shaped key.
Related: [[project_session_registers]], [[project_rarangi_messages_fleet]].
