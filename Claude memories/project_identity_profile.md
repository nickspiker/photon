---
name: project-identity-profile
description: "Identity profile: THE BLINDING (stages A+B) BUILT 2026-07-14 — handle at rest NOWHERE, party ids = pinned pubkeys, identity-DH CLUTCH egg, 64B avatar pin; stage C (profile slot + names UI) NEXT, see docs/TODO-later.md"
metadata: 
  node_type: memory
  type: project
  originSessionId: f67cf0e6-bfe4-48a0-a89b-8c3f9ce1c50b
---

Identity-profile design settled 2026-07-14, doc = docs/identity-profile.md; NOT built — RosterEntry change rides the roster rework, NFC card rides the pairing-v2 NFC transport.

Core decisions (user-confirmed):
- Handle = spoken secret, at rest NOWHERE (extends the [[project-session-registers]] model outward to friends). RosterEntry.handle today is a HONEYPOT — every friend holds the master input to your identity; replaced by a pin-set {published name, local petname, hp, pinned identity_pubkey, profile grant}.
- First-met: type the handle once → derive hp + expected identity pubkey → verify genesis → pin → discard string; all later checks (incl. every-fetch genesis re-verify) run off genesis_identity_pubkey, never re-derivation.
- GRANTS-ONLY disclosure (explicit user choice, forced by the "type 'somename', see taken, and that's it" constraint — guessable handles must not leak faces): handle-knowledge is NOT a read capability; every profile read = profile key sealed to a specific identity, subject-signed. Requester discloses first (grant rides the friend request); introductions double-consented; avatar wall migrates from handle-derived keys to grants (per-handle derived key RETIRES).
- Required name + optional fields, identity-signed, versioned; names non-unique/zero-trust; keyed two-word voca pseudonym from hp for ungranted identities; petname always wins locally.
- Profile key EPOCHED like the fleet key, sealed per-friend-identity (friend-graph analog of the fanout): rotate to ostracize — they keep granted epochs (testimony), never new ones.
- NFC bearer invite card: passive tag {hp, identity_pubkey, identity-signed token serial N} → tap = friend request quoting token + requester's grant → AUTO-ACCEPT + LOUD fleet-wide redemption notice (user chose auto+review over manual accept); revoke-by-serial = withholding future redemptions; stolen card can befriend, not join the fleet.

EXPANDED 2026-07-14 (user): full per-field contact card — name+avatar always-shared slots (content may be EMPTY: "" name / no avatar allowed — "required" = the slot is granted, not that content exists; UI must say "the handle IS the identity, fill in nothing if you like"); everything else (first/middle/last, address, lat/lon, mother's maiden, SSN, the works) optional, UNCHECKED by default, shared per-field per-contact via per-field random keys (grant = bundle of checked field keys sealed to the contact; update = new version same key → LIVE propagation; un-share = rotate field key + re-grant remainder).

BUILT 2026-07-14 (stages A+B, photon commits f3ce7e3 + 196e435, fgtw cd8d10b; all 134 tests green; binary installed): party ids = pinned identity pubkeys everywhere (identity_party_id), friendship secret = static identity x25519 as the 21st CLUTCH egg (identity_friendship_secret; siblings mix the shared seed — their pids aren't curve points; implemented INSTEAD of the grant-carried salt: no wire change, available from first-met; salt can layer later), Contact = {petname (EMPTY default — never the typed handle), published_name (renders, unfilled until stage C), avatar_pin [u8;64] = AES key ‖ FGTW lookup hash}, display_name() = petname → published → keyed_pseudonym(party_id), ContactIdentity/RosterEntry(PRST1)/CloudContact = pin-set codecs, avatar contact path fully pinned (download_avatar_pinned, caches keyed by party id). FLAG-DAY: contacts/rosters/indexes reset; UNTIL stage C contacts render as voca pseudonyms (no petname editor yet). TODO home = docs/TODO-later.md.

AUDIT run 2026-07-14 (results in the doc's Migration section): NO architectural blocker.
- Honeypots confirmed: Contact.handle + Contact.handle_hash (the latter IS the friend's identity SEED — friends hold each other's signing seeds today), ContactIdentity index, RosterEntry.handle, sibling contacts carrying our own handle.
- Identifier class (party slots, ceremony_id, friendship_id, conversation_token, chain indices) re-keys to pinned identity pubkey → friendship ids change → flag-day re-CLUTCH of all friendships.
- Secret-ingredient class (the 2 real ones): CLUTCH shared seed mixes private handle_hashes; S-blind pad context = friend's seed. Both re-key to the FRIENDSHIP SALT — random 32B carried in each side's grant, mixed both ways; STRICTLY stronger than handle-derived (guessable 'somename' made today's "private" ingredient computable by anyone).
- Avatar decrypt key (identity-seed-derived, avatar.rs) → avatar field key under grants.
- Bonus: pinned-pubkey genesis check is cheaper than seed derivation and closes today's gap where contact fleet refreshes fold WITHOUT a genesis check.

**Why:** the handle is A=1 material (identity_seed derives from it); user's framing "type 'somename' and just see it taken and that's it" became the design constraint that killed the derived-read path.
**How to apply:** build = full conversion + profile/contact screen; governed by [[project-device-sovereignty]]; grant/rotate/withhold machinery mirrors the fleet fanout in [[project-keyring-design]]; S details in [[project-token-private-identity]].
