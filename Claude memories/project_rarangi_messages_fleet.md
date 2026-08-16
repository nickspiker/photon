---
name: project_rarangi_messages_fleet
description: "rārangi message storage design — conversation=friendship_id bytes, pk=monotonic u64 counter, fleet vaults byte-identical except device crypt key"
metadata: 
  node_type: memory
  type: project
  originSessionId: f4773ae0-491a-4756-8abc-d5fb5da311f3
---

How messages (and conversation content generally) get stored in rārangi, decided 2026-06-26. Extends [[project_storage_layering]].

**Fleet = a conversation.** Your own devices talking to each other is a clutch + chain state exactly like a user-to-user or group conversation — "a singular group chat" whose participants are your devices. Same machinery: `FriendshipId::derive(sorted participant seeds)`, chain state, message rows. The ONLY per-device difference is kete's local crypt key (the `secret`). **The vault CONTENTS — addresses and plaintext — must be byte-identical across the fleet.** This is why no address may ever depend on `secret` or device identity: `vault_key(domain, scope)` and rārangi addresses are pure functions of identity seeds, never device. The device key only wraps bytes, never names them.

**rārangi internal addressing (confirmed in code):** `Db` builds string keys (`keys::row(table,pk)` = `"row/{table}/{pk}"`) → `Store::put(key:&str)` → kete `write` → `write_addr(blake3::derive_key("photon.storage.entry.v0", key.as_bytes()))`. So every rārangi string key is hashed to a 32-byte BLAKE3 address inside kete. The string is a surface convenience; the keyspace is fixed 32 bytes (the "infinite string / collision" intuition was right). EXCEPTION: the catalog (`catalog.rs`, `keys::catalog(table,shard)`) stores the pk list VERBATIM as VSF values so `list(table)` can enumerate (manifestus is non-enumerable). So: TABLE name is address-only (hashed away, can be pure bytes), PK must round-trip (lives in the catalog).

**The message storage design:**
- Conversation TABLE = `friendship_id` BYTES (32). Address-only → passes as raw bytes, no text encoding. `FriendshipId::derive` works for 1 (self-notes), 2 (DM), N (group), and the fleet (own devices) alike.
- Per-message PK = a MONOTONIC u64 COUNTER. First message = 0, then 1, 2… VSF integers are unbounded so no width problem. Fleet-stable because a conversation is an ORDERED sequence delivered in the same order on every device — message N is message N everywhere. Gives chronological list() for free. NOT a hash/content-address (over-engineering — the data is just a list).
- Catalog generalized to be pk-TYPE-AGNOSTIC: store the pk as its native VSF type — `u6` for a counter, `x` for a genuine string id. (Today catalog.rs hard-codes `VsfType::x`.)

**API shape the user asked for:** "expose both, be Rusty about it — `Option<&str>` when we don't have a string." Honest form given VSF carries type tags: a `Pk` enum (Str / Int(u64) / Bytes) round-tripped through the catalog as the matching VSF value; table accepted as bytes. Old string `put_row(table:&str, pk:&str)` stays for rārangi's genuine string callers (and rārangi's own tests / music-vault example).

**Per-peer message model is dead:** old `contacts.rs save_messages/load_messages` keyed by `their_identity_seed` (per-peer) was wrong for groups AND was content in the wrong layer (hand-rolled kete blob). Replaced by conversation rows. `Contact.friendship_id` is `Option` (set after CLUTCH) — but derive it EARLY from participant seeds (deterministic, no ceremony needed) so messages are always keyed by conversation. Live callers: handle_query.rs:557 (load), photon_app.rs:2885/4901/5083 (save). app.rs callers are DEAD CODE (ui/app.rs + ui/mouse.rs are not `mod`-declared — legacy Android compositor, cleanup target).
