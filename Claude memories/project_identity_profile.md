---
name: project-identity-profile
description: "Identity profile: THE BLINDING (stages A+B) BUILT 2026-07-14 — handle at rest nowhere, party ids = pinned identity pubkeys, grants-only disclosure; stage C (profile + names UI) next; docs/identity-profile.md"
metadata:
  type: project
---

Canonical: docs/identity-profile.md (design + migration audit).
Decisions (Nick, 2026-07-14):
- The handle is a spoken secret, at rest NOWHERE (extends [[project_session_registers]] to friends). A contact is a pin-set {published name, local petname, hp, pinned identity_pubkey, profile grant}; first-met = type the handle once → derive → verify genesis → pin → discard the string.
- GRANTS-ONLY disclosure: knowing a handle is not a read capability ("type 'somename', see it taken, and that's it" — guessable handles must not leak faces). Every profile read = a key sealed to a specific identity; the requester discloses first; introductions are double-consented.
- Per-field contact card: name + avatar slots always granted (content may be empty); every other field optional, unchecked by default, shared per field per contact via per-field keys (un-share = rotate + re-grant the rest). The profile key is epoched like the fleet key.
- Names are non-unique and zero-trust; ungranted identities show a keyed two-word voca pseudonym from hp; the petname always wins locally.
- NFC bearer invite card: tap = friend request quoting a signed serial → auto-accept + loud fleet-wide notice; revoke by serial.
BUILT stages A+B (photon f3ce7e3 + 196e435, fgtw cd8d10b): party ids = pinned identity pubkeys (`identity_party_id`); friendship secret = static identity x25519 as the 21st CLUTCH egg (`identity_friendship_secret`); Contact = {petname (empty default), published_name, avatar_pin [u8;64]}; display_name() = petname → published → keyed pseudonym.
Stage C (profile slot, grants, names/profile UI) and the NFC card were next as of 2026-07-14 — check the code before assuming either way.

**Why:** the handle derives identity_seed; any friend holding it held the master input to your identity.
**How to apply:** never store or send a handle string; governed by [[project_device_sovereignty]]; grant/rotate machinery mirrors the fleet fanout.
