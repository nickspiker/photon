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

### What lands on the wall

One object, self-contained: the wrap set travels **with** the ciphertext, so a reader needs only the blob id plus a KEK it already has. Nothing else has to be fetched, and no side channel carries a secret.

```
blob/<base64url(blob_id)>  →  VSF document
  section "blob"
    version   z{N}                     -- binary numeral, never an ASCII digit in a tag
    blob_id   hb{32}
    wrap      [ epk ke{32}, commit hb{32}, ct v'e' ]   -- repeated, one per recipient, UNLABELLED
    content   v'e'{ AEAD(DEK, plaintext) }
```

**Wraps carry no recipient label.** A reader recomputes the key-commitment for each wrap and opens the one that matches — the same self-selection the fan-out uses (`fanout_keys`). The object therefore leaks a recipient *count* and nothing else: no pubkeys, no handle proofs, no idea who the readers are.

### Wrap derivation

Per wrap, mirroring `fanout_keys` so there is one construction to review, not two:

```
shared  = KEK-specific secret
          (fleet key for our devices; CLUTCH pair secret for a friend)
okm     = BLAKE3-XOF( DOMAIN_TEXT ‖ [version u8] ‖ blob_id ‖ epoch_le ‖ recipient_ed ‖ epk ‖ shared )
aead_key, commit = okm[0..32], okm[32..64]
ct      = ChaCha20-Poly1305(aead_key, nonce=0, DEK)
```

`epk` is a fresh ephemeral X25519 public per wrap, which is what makes each wrap key unique and therefore what makes the fixed zero nonce safe — the identical argument the fan-out already documents. `commit` is the key-commitment (defeats the invisible-salamander split) **and** the recipient selector. Binding `blob_id` and `epoch` stops a wrap being spliced onto a different blob or an older version.

---

## Operations

**Publish / update.** Mint a fresh DEK, encrypt the content once, wrap for every current reader, upload one object. Cost is one upload plus ~100 bytes per reader.

**Add a reader** (new friend, new device). Rewrap the *existing* DEK for them and rewrite the wrap set. The ciphertext is untouched — no re-upload of content.

**Remove a reader** (friend removed, device departed). Drop their wrap. For a device, the fleet-key rotation that already fires on membership change does the work: the departed device is not a fan-out target next epoch, so it cannot derive the KEK, so it cannot open even a wrap it still has bytes for.

**What removal does NOT do**, stated plainly because the honest limit belongs in the spec and not in a surprise: a reader who already fetched and decrypted **keeps that plaintext forever**. Rotation and rewrapping stop *future* versions only. This is the same SOFT/HARD line [contact-system.md](contact-system.md) already draws — the crypto is unforgeable about the future and powerless about the past.

**Burn and republish** (the device-handoff case). Giving a phone to another person is the one case where "future only" is not enough: that device already holds the current DEK. Mint a new DEK, re-encrypt the content, rewrap for the surviving readers, delete the old object. This is a deliberate, explicit operation — cheap for a 22 KB avatar, and the reason it is a *policy step* rather than an automatic one is that for a large attachment it is not cheap at all.

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
- Property: rewrapping (add/remove a reader) leaves the ciphertext byte-identical — the whole point of the tier.
- Live two-device + one-friend: set an avatar, confirm both own devices and the friend render it; remove a device, rotate, confirm it can no longer open the CURRENT object; burn-and-republish, confirm the removed device fails on the new one too.
- The acceptance test that applies to every field this codebase adds anywhere, learned the hard way on 2026-08-01: **wipe a device and confirm it comes back.**
