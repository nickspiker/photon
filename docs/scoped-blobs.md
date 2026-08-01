# Scoped blobs — one ciphertext, many keyholders

> Status: **DESIGN.** Written 2026-08-01 after the bearer-pin model produced three distinct data-loss bugs in a single day.
> This is the concrete instance of [fleet-vault-security.md](fleet-vault-security.md)'s "rotate keys, not data" tier: that doc says *why* the hierarchy exists, this one says exactly what goes on the wire for a shared file.
> First consumer is the avatar. Attachments and any other "several people may read this one object" case ride the identical mechanism — nothing here is avatar-shaped.

---

## The problem this replaces

Today an avatar is protected by a 64-byte **pin**: `pin[0..32]` is the decryption key, `pin[32..64]` is the FGTW storage address. One secret grants both *where it lives* and *how to read it*, it is identical for every friend, and it is handed out over pongs and the roster.

Everything wrong with that showed up in one day (2026-08-01):

- **Losing the pin loses the content.** The wall copy is addressable by nothing else. A wiped device with unread fleet settings minted a fresh pin and orphaned the published avatar — twice.
- **Two ciphertexts of one image.** The vault copy is seed-keyed, the wall copy is pin-keyed. Serving a friend the vault bytes handed them something keyed to a secret only we hold (`aead::Error` on a clean transfer), and own-avatar recovery searched the seed-derived address for something published under the random pin.
- **Bearer semantics.** Possession is authorization. Every friend holds the *same* secret, so any of them can pass it to a stranger and nothing says who did.
- **Revocation is all-or-nothing.** Cutting off one departed device forces a fresh pin, so every friend must re-fetch.

The fix is not a better pin. It is to stop making the pointer secret.

---

## The model

**Encrypt once. Wrap the key many times.** Exactly what the per-member fan-out already does with the fleet key — this is that mechanism, generalized from "a key" to "any blob".

- **DEK** (data key): random 32 bytes, minted per blob **version**. Encrypts the content, once.
- **KEK** (key-encrypting key): one per recipient class, all of which already exist —
  - **our own devices** → the **fleet key** (itself fan-out-delivered per device, and egged as of 2026-08-01, so post-quantum).
  - **each friend** → the **CLUTCH pair secret** for that friendship (post-quantum, per-friend, already derived at ceremony completion).
- **Blob id**: a random 32-byte identifier, and — the load-bearing change — **not a secret**. It is the storage address and nothing more. Learning it grants nothing, because reading requires a wrap you can only open with a KEK you already hold.

The old pin's two jobs are now separated: the id says *where*, the wrap says *whether*.

### Where the wraps live — one private slot per reader

The wrap set does **not** travel with the ciphertext. If it did, the object would enumerate its own readers: anyone who fetched it would learn how many people can read it, and that count is a fact about your life that no one is owed.

Instead each reader has a **private slot** at an address only the two of you can compute:

```
slot_addr = BLAKE3( SLOT_DOMAIN ‖ [version u8] ‖ kek_secret ‖ purpose )
```

where `kek_secret` is the CLUTCH pair secret for that friend, or the fleet key for our own devices. The slot is tiny and holds only a pointer and a key:

```
blob/<base64url(slot_addr)>  →  VSF document, device-signed
  section "slot"
    version   z{N}
    blob_id   hb{32}          -- which ciphertext to fetch (NOT secret)
    wrap      v'e'{ AEAD(slot_key, DEK) }
```

The content itself is one object at a non-secret address, encrypted once:

```
blob/<base64url(blob_id)>  →  VSF document
  section "blob"
    version   z{N}
    content   v'e'{ AEAD(DEK, plaintext) }
```

So a reader derives their slot address, finds a ~100-byte record (or nothing at all), unwraps the DEK, and fetches the single shared ciphertext. **One ciphertext, N tiny private pointers** — putting the content in each slot instead would mean N copies, which is merely wasteful for a 22 KB avatar and ruinous for an attachment.

Slot addresses are mutually unlinkable and reveal nothing about who holds them: a stranger cannot tell your twelve slots from twelve unrelated people's, and cannot derive an address without already holding the secret that would let them read anyway.

**Squatting.** An address is only computable by the two parties, so it cannot be found by guessing. If one leaks, an attacker could occupy it — which is why a slot is device-signed and the reader rejects anything not signed by the expected device key. That reduces the worst case to a denial of service against one reader, never a spoof.

### What still leaks, exactly

**A count, at publish time, and nothing else.** FGTW authenticates writes, so when you publish an avatar it observes one identity writing N slots in one burst. It does not learn who they are, the addresses are unlinkable, and reads are unlinkable. Since a publish is rare — an avatar change, not a message — this is a small and bounded signal. It is recorded here rather than called solved.

If it ever needs closing, the lever is decoupling writes from publish time: write a slot lazily when a reader actually asks, or pad to a fixed slot count so the number carries no information.

## Operations

**Publish / update.** Mint a fresh DEK, encrypt the content once, upload it under a fresh blob id, then write one slot per current reader. Cost is one upload plus ~100 bytes per reader.

**Add a reader** (new friend, new device). Write one new slot: the *existing* DEK wrapped to their secret, pointing at the existing blob id. The ciphertext is untouched, and no other reader's slot changes.

**Remove a reader** (friend removed, device departed). Mint a fresh DEK, re-encrypt the content to a **new blob id**, and write slots only for the survivors. The removed reader is not locked out of an address they still know — the object moved out from under them, and their stale slot points at something that no longer exists.

Deleting the old object is **not required**, and nothing here depends on it. That ciphertext is opaque to everyone except the readers who were already granted it, and those readers already hold the plaintext — so an orphan left on the network leaks nothing that was not already given away. Hosts may reclaim it under their own storage policy; a signed tombstone is available as a courtesy hint to help them, never as something correctness rests on. This matters because there is no fgtw.org in the end state: a design that requires someone to obey a delete is a design that breaks the day the server retires.

For a departed *device* the fleet-key rotation that already fires on membership change does the same work one level up: it cannot derive any slot address under the new key, so it cannot even find a slot to try.

**What removal does NOT do**, stated plainly because the honest limit belongs in the spec and not in a surprise: a reader who already fetched and decrypted **keeps that plaintext forever**. Republishing stops *future* versions only. This is the same SOFT/HARD line [contact-system.md](contact-system.md) already draws — the crypto is unforgeable about the future and powerless about the past.

### Availability — friends hold it, the wall only distributes it

A reader caches the decrypted content locally the first time it fetches. **The wall is a distribution cache, not the origin**: the origin is the publishing device, and the working copies live with the people who actually need them.

The consequence that decides the retention model: go off-grid for a year and your avatar still renders for every friend who already has it, because none of them need the network to show it. There is therefore NO lease, NO expiry, and no refresh obligation — an identity that decays because its owner went hiking is broken, and "refresh or lose your face" is the wrong failure. Hosts apply whatever garbage collection suits them; the protocol never punishes absence.

What being away DOES cost is granting NEW access: writing a slot needs the publisher's key, so a friend made while offline gets their slot when the publisher next comes back. Granting requires presence, which is correct.

**Burn and republish** (the device-handoff case). Giving a phone to another person is the one case where removing a reader is not enough on its own: that device already holds the current DEK, so the current content must move too. Mechanically identical to removing a reader — new DEK, new blob id, survivors' slots rewritten, old object deleted — but worth naming as its own deliberate step, because for a large attachment the re-encryption is the expensive part and should be a choice rather than a reflex.

**Attribution.** Each reader has a distinct wrap, so we always know exactly who was *issued* access. We cannot tell which of them leaked a plaintext — that would need a per-reader DEK and therefore a per-reader ciphertext, which is affordable for a small avatar and absurd for a large file. Not doing it; recorded so nobody assumes otherwise.

---

## What this deletes

- The 64-byte bearer pin, `profile.avatar_pin`, and the fleet-setting plumbing that syncs it.
- `avatar_pin_from_seed` and the whole seed-derived addressing lineage.
- The **second ciphertext**: there is one encrypted object, and our own devices are simply another set of wrap targets. The vault copy becomes a local cache of the same bytes, not a differently-keyed twin.
- `avatar_vsf_for_friend` (the re-encrypt-on-serve workaround added 2026-08-01) — with one ciphertext there is nothing to convert.
- The rotate-on-set dance and its "new pin announced before the upload lands" race.

The `avatar_pin` field stays in the roster and pong for exactly one release as a **blob id** carrier (non-secret now), then goes.

---

## Migration

Flag-day, consistent with every other wire change this week: a device publishes its avatar in the new form on first launch, readers that see a new-form object use it, and the old pin path is deleted in the same release rather than kept as a fallback. Losing an avatar for one publish cycle is acceptable; carrying two key models is not — that duality is what caused the bugs this design exists to end.

---

## Build order

1. `fgtw` (shared crate): `scoped_blob.rs` — wrap/unwrap + the VSF document codec, pure functions, no I/O. Reuses the `fanout_keys` construction; the worker never needs it (blob storage is opaque bytes to FGTW).
2. photon: publish/fetch/cache paths for the avatar; delete the pin.
3. photon: wire the friend KEK to the stored CLUTCH pair secret (`storage::fanout_pairs`), and the device KEK to `fleet_key_cached`.
4. Attachments adopt it unchanged when they next need multi-reader access.

## Verification

- Unit: round-trip for a device reader and a friend reader; a non-reader with a valid KEK of the wrong class fails; a wrap spliced onto another blob_id fails; a wrap from an older epoch fails; tampered ciphertext fails its tag; wrap-count sanity bound.
- Property: ADDING a reader leaves the ciphertext byte-identical (one new slot, nothing else touched); REMOVING one produces a new blob id and deletes the old object.
- Live two-device + one-friend: set an avatar, confirm both own devices and the friend render it; remove a device, rotate, confirm it can no longer open the CURRENT object; burn-and-republish, confirm the removed device fails on the new one too.
- The acceptance test that applies to every field this codebase adds anywhere, learned the hard way on 2026-08-01: **wipe a device and confirm it comes back.**
