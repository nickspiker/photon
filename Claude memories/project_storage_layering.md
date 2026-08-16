---
name: project_storage_layering
description: Storage layering — vault vs chain-state vs conversation DB; file/tree paths were half-assed into the flat vault and are being de-stringed
metadata: 
  node_type: memory
  type: project
  originSessionId: f4773ae0-491a-4756-8abc-d5fb5da311f3
---

The three storage layers in photon, and the migration in flight (started 2026-06-26):

1. **Vault** = kete::FlatStorage. One `.vsf` file per handle, namespaced by `blake3(my_seed)`. The vault filename is the ONLY base64/encoded name allowed on disk under `~/.config/photon/`. Everything else is a flat 32-byte keyed entry.

2. **Chain state** = the crypto ratchet (`FriendshipChains`: per-participant chains, conversation_token, hash-chain heads). Small, security-critical, mutated per message. Lives as a raw kete entry keyed by the conversation. **Storage != chain state** (user's words) — chain state is machinery, not content.

3. **Storage / content** = messages, attachments, call recordings, audio. This is **rārangi's** job (the conversation database: rows + content-addressed blobs, built and tested but NEVER wired into photon — zero `use rarangi`, no Cargo dep). Messages were hand-rolled as kete blobs under `contacts/{hex8}/messages` — wrong layer AND wrong (per-peer) model.

**Root cause the user named:** file/tree-based storage got half-assed into the flat vault. Photon was written against a filesystem mental model (paths like `contacts/{hex8}/state`, `friendship/{hex8}/chains`, avatar *files*) and those tree-shaped string keys were passed straight through kete's `write(key: &str)`, which hashes them to addresses so it works — but the design underneath is still a directory tree. The `hex::encode(&seed[..8])` and base64 avatar filenames are binary identity the caller already held, laundered through text. rārangi's string keys are the OPPOSITE — correct, because table/pk/seq are genuinely strings (not encoded binary), and never become filenames. So rārangi stays string-keyed; photon's DIRECT kete use moves to byte-addressed.

**Canonical key rule (no paths, no hex, no base64):**
`vault address = blake3(domain_word, scope_bytes)`
- self/global: scope = my_vault_seed — e.g. avatar, settings, contacts(index)
- per-peer: scope = their_seed — e.g. their avatar, state, keypairs, slots
- per-conversation: scope = friendship_id (already `blake3("PHOTON_FRIENDSHIP_v1", sorted_participant_seeds)`, works for 1/2/N) — e.g. chains (state, in kete), messages (content, in rārangi)

**Migration pieces (ordered):** (1) kete `*_addr(&[u8;32])` twins alongside string API — DONE, enc key now address-bound. (2) photon `vault_key(domain,scope)` helper. (3) avatar→vault. (4) de-string contacts keys. (5) de-string chain-state key. (6) wire rārangi for messages (rows keyed by conversation). Attachments/recordings via rārangi blobs = LATER. No data migration needed — photon is nuked on all machines (clean slate).

Relates to [[project_session_registers.md]] (vault/network roots separate), [[project_vault_roadmap.md]], [[project_identity_storage_model.md]].
