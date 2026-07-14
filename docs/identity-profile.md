# Identity profile — names, grants, and the spoken handle

Status: DESIGNED 2026-07-14, not built.
The RosterEntry change rides the roster rework; the NFC invite card rides the pairing-v2 NFC transport work (shared reader plumbing).
Governing rule: sovereign records — the subject signs, others verify or withhold; pending expires, completed is permanent testimony; ostracism, never erasure (docs/pairing-v2.md carries the full statement).

## The shift

The handle is A=1 authentication material: `identity_seed` derives from it one-way, so whoever holds the string holds identity co-signing power.
It should therefore exist at rest NOWHERE — not the worker, not friends' vaults, not even your own vault — only in your head and in a one-time entry field.
Own devices already live this way (the tohu session registers hold `{identity_seed, vault_seed, handle_proof}`, never the string).
Friends' devices do NOT: `RosterEntry.handle` gives every friend a durable copy of the master input to your identity, making your attack surface the union of your social graph's device security.
This design completes the register model outward: friends keep a pin-set of derived, verification-only material, and the human-facing layer becomes a published profile.

## The pin-set (what a contact stores about you)

`{ published name (adopted), petname (local override, always wins on the holder's screen), handle_proof (routing), identity_pubkey (the genesis pin), current profile grant, optional profile fields }`.
First-met by spoken handle: type it once → derive hp + the expected identity pubkey → verify the chain genesis under it → PIN the pubkey → discard the string.
The derivation IS the verification — this is TOFU→pin without the blind first use.
Every later trust check, including the every-fetch genesis re-verification, runs off the pinned pubkey (`genesis_identity_pubkey`), never off re-derivation.
A compromised friend device then leaks who you know, not how to be you.

## Disclosure: grants only (decided 2026-07-14)

Typing a handle reveals existence ("taken") and addressing — and NOTHING else.
Handles are guessable ('alex'), so handle-knowledge must not be a read capability: every profile read is a **grant** — the profile key sealed to a specific identity pubkey inside a record the subject signs.
Nobody sees your face and name except across an edge you consented to; this is the anti-doxx default, stated plainly, and it is the feature.

Grant paths:
- **Friend request**: the requester discloses FIRST — their grant rides the signed request, so the recipient can render who's knocking while trusting nothing. Acceptance reciprocates the grant; mutual consent completes and both walls open.
- **Introduction**: double-consented — C proposes, then A and B each seal a grant to the other. C can vouch but can never disclose A's profile to B; only A signs A's disclosures.
- **Invite token**: the NFC card below.

First-met confirmation under grants-only: you verify WHO by the genesis pin and by the fact that they spoke the handle to you — not by peeking at a face.
The avatar wall migrates from handle-derived keys to the profile grant: same storage and v'e' encryption, new key source; the per-handle derived key retires.
Ungranted identities render as the keyed two-word voca pseudonym from hp (the `device_name_default` pattern one level up) — "quietFalcon", never a blank, never a face.

## Profile key epochs

A random key per epoch, sealed to each current friend's identity pubkey — the friend-graph analog of the fleet-key fan-out, same grant/rotate/withhold machinery one level up.
Rotate on falling-out (they keep every epoch they were granted — permanent testimony — and never see a new one), on token revocation, or at will.
Fleet key sealed to member devices; profile key sealed to friend identities; both governed by verify-or-withhold.

## The profile — required slots, optional everything (expanded 2026-07-14)

The profile is a full per-field contact card the user owns: preferred name and avatar are the ALWAYS-SHARED slots; everything else — first/middle/last name, address, lat/lon, mother's maiden name, SSN, the whole book — is optional, UNCHECKED by default, and shared per-field per-contact.
"Required" means the SLOT is always granted, not that content must exist: the name may literally be `""` and the avatar absent — the handle IS the identity, so the user need fill in NOTHING (the profile screen says so explicitly); empty slots render as the keyed voca pseudonym / placeholder ring, same as an ungranted stranger.
Photon never asks for any of it — the slots exist because the user owns the record, not because the system wants the data.

Per-field mechanics:
- Each field is sealed under its own random FIELD KEY; a contact's grant is the bundle of checked fields' keys sealed to their identity pubkey.
- Updating a field = a new identity-signed version under the same field key — every contact it's shared with gets the update live (move house once, everyone you shared your address with has the new one).
- Un-sharing a field from a contact = rotate that field's key and re-grant to the remaining checked contacts; they keep the last value they were given (testimony), and never see another.
- Checking a field for a contact = seal the current field key to them; they read the current version immediately.

Names are non-unique, mutable, and carry ZERO trust: collisions are a rendering problem (disambiguate by avatar/colour/attestation lineage), impersonation is dead on arrival because the pinned key and mutual consent carry the trust, and the local petname always beats the published name on your own screen.

## The friendship salt (audit consequence, 2026-07-14)

Each side's grant carries a random 32-byte FRIENDSHIP SALT; both salts mix into the friendship's key derivations.
This replaces the mutual-handle-knowledge secrecy ingredient the CLUTCH stack uses today — and replaces it with something STRICTLY stronger: today's "private" `handle_hash = BLAKE3(handle)` is only as private as handle entropy, so a guessable handle ('alex') makes the ingredient computable by anyone; a grant-carried random salt is per-relationship, full-entropy, and revocable.

## NFC invite card — the bearer token

A cheap passive tag (no HCE, no power) carrying `{ hp, identity_pubkey, invite token }` where the token is identity-signed: "the bearer of serial N may friend me", with optional expiry and max-redemptions.
Tap → the reader gets addressing + pin + token → sends a friend request quoting the token and enclosing its OWN grant → the subject's devices honor valid serials with auto-accept and a reciprocal grant → both sides added, one tap, no typing.

- **Auto-accept + loud review** (decided 2026-07-14): the tap is the consent — you handed over the card. Every redemption notifies the WHOLE fleet ("card N redeemed by quietFalcon → now Alex"), reviewable after the fact; ostracize + rotate if the hand-off wasn't yours. Act fast, review loud — the two-phase pattern's sibling.
- **Revocation = withholding**: the fleet stops honoring serial N; nothing already completed is erased — redeemed friendships stand until individually ostracized via epoch rotation.
- **Bearer risk, accepted and bounded**: a stolen or cloned card can befriend, not join the fleet; every use is visible; serials die on demand.

The card is friend-add by proximity — the exact analog of device-add by proximity: candidate delivered by tap, consent signatures on both sides, the same candidate-and-selector philosophy one layer up.

## What the handle still does — and no longer does

Still: existence probe (type 'alex', see taken, and that's it), addressing (hp), aiming a friend request, deriving your own registers, enrolling your own devices.
No longer: a display name, a read capability, a stored field anywhere.

## Migration — audit results (run 2026-07-14)

Verdict: NO architectural blocker; two crypto derivations need the friendship salt, everything else is identifier re-keying and storage field swaps.

Legitimate one-time handle uses (already correct, keep):
- `handle_query` Probe/FirstAttest — own handle at entry; explicitly "the one moment the handle string exists"; resume already skips it.
- `submit_join_step` (own handle at fleet join), the self-handle check, the join-screen device-name derivation, `photonlog` dev tooling.

Storage honeypots (the conversion targets):
- `Contact.handle` (string) AND `Contact.handle_hash` — the latter IS the friend's identity SEED (`to_identity_seed(handle)`), persisted per contact: friends hold each other's signing seeds today, which is worse than the string.
- `ContactIdentity {handle_proof, handle}` — the vault's contact-list index carries strings.
- `RosterEntry.handle` — fleet-sealed on FGTW, synced to every member device.
- Sibling contacts carry OUR OWN handle string for peer-row matching (peer rows are hp-keyed; the string is droppable).

Runtime uses that must re-key:
- IDENTIFIER class — party-slot indexing, `ceremony_id`, `friendship_id`, `conversation_token`, chain indices: all opaque sorted 32-byte party ids that are `handle_hash` for friends. Replacement: the pinned identity pubkey. Consequence: friendship ids change → flag-day re-CLUTCH of every friendship (conversation tables re-key; at current scale, re-establish rather than migrate).
- SECRET-INGREDIENT class (the two real ones): the CLUTCH shared seed deliberately mixes "private handle hashes" (clutch.rs — "only known to parties who know the plaintext handle"), and the S-blind pad context is the friend's identity seed (blind.rs, deliberately seed-not-string for rename-proofing). Both re-key to the friendship salt above — a strict upgrade, see that section.
- Avatar decrypt key is identity-seed-derived (avatar.rs) → becomes the avatar field key under the grant model.
- Contact-chain genesis verification: the pin makes it CHEAPER — compare `genesis_identity_pubkey()` to the pinned key, no seed derivation; and it can now run on every contact fleet refresh (today's refresh folds without a genesis check — the pin closes that gap as a side effect).

`RosterEntry` drops `handle`, gains the pinned identity_pubkey + profile-grant material + petname; rewrite in place at current scale, riding the roster rework.
The deployed avatar wall keeps working thru the transition; grant-keyed profile objects land beside it and the derived-key path retires at the same flag-day as the re-CLUTCH.
