---
name: project_rarangi_messages_fleet
description: "rārangi message storage: table = friendship_id bytes; fleet = a conversation; addresses never depend on the device, so fleet vaults hold byte-identical contents"
metadata:
  type: project
---

Decided 2026-06-26, extends [[project_storage_layering]].
- **Fleet = a conversation:** your own devices are participants like any friendship; `FriendshipId::derive(sorted participants)` covers self-notes, DMs, molecules and the fleet.
- **Addresses never depend on the device:** `vault_key(domain, scope)` and rārangi addresses are pure functions of identity seeds; the device key only wraps bytes, never names them — vault contents are byte-identical across the fleet.
- rārangi string keys (`row/{table}/{pk}`) hash to 32-byte BLAKE3 addresses inside kete; only the catalog stores pks verbatim (manifestus is non-enumerable), so the TABLE name can be raw bytes but the PK must round-trip. The catalog stores a pk as its native VSF type (`Pk` enum Str/Int/Bytes).
- Conversation TABLE = `friendship_id` bytes (32), derived early from the participants (no ceremony needed) so messages are always keyed by conversation.
- Row keys: the original plan was a monotonic u64 counter; [[per-device-lanes]] (2026-08-13) set the at-rest row key to BE(eagle_time) ‖ blake3(content)[..8] so byte order = canonical order — that one wins.

**Why:** fleet copies must be interchangeable; a device-dependent address would fork them.
**How to apply:** never mix device identity or the kete `secret` into any storage address.
