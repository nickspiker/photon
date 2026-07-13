# Device trust, removal, and identity recovery

The consent-only membership model, the friend-side trust override, and custodian-authorized total-loss recovery.
Status: DESIGN, decided 2026-07-12. This SUPERSEDES the remote-removal model — see "What this replaces" at the end. Not built yet.

## The spine: three rules

1. **Chain membership is consent-only.** A device joins by bilateral pairing (both the joining device and an existing member sign) and leaves only by its OWN signature. No member can remove another. This binds everyone equally — you, an attacker holding one of your devices, and any custodian quorum.
2. **Effective trust = member AND friend-accepts.** Chain membership is necessary but not sufficient: each friend independently decides whether to serve/answer/deliver to a given device, and can refuse one regardless of what your chain says. This is the routing-layer override, reversible and friend-controlled.
3. **Custodians batch, never seize.** A K-of-N custodian quorum can authorize an identity succession (total-loss recovery) and vouch to friends, but holds no secret and cannot reach into your fleet to manage devices any more than you can.

Cryptography defends the fleet (rules 1); friends defend the identity (rule 2); custodians make the social layer fast without gaining seizing power (rule 3).

## The handle is a public name, not a key

Foundational, because it shapes everything below.
`identity_seed = BLAKE3(x(handle))` is a CHEAP hash; your friends type your handle to add you and store it for display, so every friend can derive your identity_seed.
The handle is therefore not a secret — it is your public address.
What actually makes you *you* is `device_secret` (per-device, from the platform oracle, never derived from the handle — this is what gates the vault) and **S** (the CSPRNG private identity, never at rest, reconstitutable only through a friend's blind-serve).
Being you = device + S + being a trusted fleet member.
The boot-lock works because the session stores the derived `identity_seed` register, never the handle string, so a rebooted stolen device cannot re-derive identity_seed, cannot re-attest, and bricks — a defense that needs no handle secrecy.

## Removal: consent-only

A device leaves the fleet only by signing itself out. There is no remote-removal op.

**Why.** Remote removal (the shipped model, where any member can remove any member) hands a thief who holds one of your devices the power to evict your *real* devices and rotate you out of your own identity — the sharpest edge of the un-revocable-device problem. Requiring the removed device's own signature kills that attack: the only device an attacker can sign out is the one they stole, which is a favour.

**The chain becomes consent-symmetric.** Adds are already bilateral (pairing needs the joining device); with self-signout, every voluntary membership transition is signed by the device it happens to. The chain records only consent facts — bilateral adds and self-signed departures, nothing else.

**Accepted cost: lost hardware is burned forever.** A device in a lake can't sign itself out, so it stays a member (locally tombstoned — a UI annotation, zero protocol weight). Because device keys are fingerprint-deterministic, that lost device is also permanently worthless as photon hardware to anyone who finds it (no identity to bind, empty vault). Stolen goods have no resale value in this economy; your own losses are equally permanent. This is the microwave doctrine — the only true revocation is physical destruction of the key; everything else is changing who *accepts* it.

## The un-strandable gap, and the override that fixes it

**The gap (found via the additions question, 2026-07-12).** `Contact::knows_device` is currently a pure function of the folded chain — post-fold it is exactly `fleet_members.contains(dev)`. A friend therefore auto-trusts EVERY current member of your chain, with no way to refuse one. Combined with consent-only removal, a compromised member is not just unremovable — it is *un-strandable*: trust follows the fold, and the fold can't shed a device. And a compromised member can *plant* new devices (bilateral pairing, it holds a member, a fresh device passes the one-owner guard), which are then equally unremovable and auto-trusted. Planting and the stolen device are the same hole, opposite polarity.

**The fix (must ship WITH consent-only, or "can't evict but can out-key" is a promise the code can't keep).** A friend-side downward override:

```
effective_trust(dev) = folded_member(dev) AND NOT friend_locally_refused(dev)
```

A friend can unilaterally, reversibly refuse a specific device pubkey regardless of chain membership.
One knob neuters stolen and planted devices identically — deaf to S, cut from new braid, no delivery — while they remain permanent chain members (rule 1 stays pure).
The refusal is friend-controlled, so an attacker with identity power can't override it; it is set out of band or via a "reported stolen" signal (see fleet-inbox.md).
Residual is cosmetic: your fleet roster shows refused-but-present ghost members; hide locally-refused devices in the UI.

## Identity reclaim without total loss (the stolen-but-you-still-have-a-device case)

S is friend-blinded and reconstitutable only when a friend serves the blind, and friends serve only to devices they accept (the override above).
So identity reclaim after theft is: reach each friend out of band, they set the stolen device to refused, and you re-mint S (new `s_id` epoch) and re-deposit with online friends.
The stolen device's held S becomes the superseded epoch; its authorship degrades to unverified friend by friend, at the speed of your social reach.
Message confidentiality is a separate, per-friendship concern: the stolen device keeps decrypting new inbound from each contact until that friendship's braid is re-keyed (re-CLUTCH), which is O(contacts) with no fleet-level shortcut — prioritize sensitive contacts.
Custodians batch all of this into one K-of-N vouch instead of you personally reaching everyone.

## Total-loss recovery: custodian-authorized supersession

When every device is gone, you have no member device to pair a new one and can't remove the dead ones. The resolution is to SUPERSEDE the whole chain, never edit it.

**Foundational choice — quorum, not secret-share.** Custodians hold no secret and no Shamir share of your identity; they hold the STANDING to co-sign a succession attestation. K-of-N signing "old identity → new identity" is a one-time bridge, not standing power. Reject secret-reconstruction: K compromised custodians reconstructing your identity would silently *become* you forever and read everything. Under the quorum model, a rogue K is a LOUD, alerted, reversible succession *attempt* that leaks nothing.

**Always custodian-gated, never identity-key/handle-alone.** Because the handle is public (every friend holds it), handle-or-identity-key-alone supersession would let any friend who knows your handle hijack you the moment your live chain (which blocks re-genesis via first-come) is being replaced. Custodian-gating is exactly what elevates "knows your handle" (everyone) to "authorized to move your identity" (a quorum). No custodians designated = total loss is total (unlinked fresh start, no friend migration) — the honest pre-provisioning cost.

**Always same-handle.** You never forget your handle (it's memorable), so recovery re-attests the same handle_proof: the old chain with its dead devices is superseded, a fresh genesis takes its place at the same address, and friends don't re-address you — they accept your recovered device-set and new S.

**Mechanics.**
- Custodian designation: an identity-signed op IN the chain (persists on FGTW through device loss — you lose devices, not the chain), naming N custodians and threshold K.
- Succession attestation: K-of-N custodian signatures over old→new, nunc-timestamped.
- Worker: verify K sigs against the designated set, mark the old chain superseded (whole-identity supersession, NOT a device removal), point to the new genesis.
- Friend migration: custodian-AUTHORIZED but friend-CONFIRMED (a prompt, never a silent swap); carries history and trust level; re-CLUTCH for a fresh braid.

**This is also the stolen-everything answer.** If a thief holds all your devices with live sessions, don't fight them on the old chain — supersede off it, custodians authorize, friends migrate, and the thief is left holding a dead identity every friend now refuses.

**Open:** commitment(hash) vs plaintext custodian set (trust-graph privacy); the friend-migration history-reassociation UX.

## What this replaces

- **device-lifecycle.md §Remove** (remote `unbind_device`, "any single member may Add and Remove", "remote removal of a LOST device is the point") — superseded by consent-only self-signout. The `unbind_device`-another-device op is retired; `clean_device_for_reuse` (a device shedding ITSELF) is the surviving self-signout pattern.
- **rekey-threat-model.md §revocation** ("revocation-that-works = remove + rotate", fold-respecting trust dropping a removed device) — superseded by the friend-side override. Fold-respecting trust stays as the *membership* signal, but effective trust gains the `friend_locally_refused` term, and eviction no longer depends on the chain shedding the device.

Both of those describe SHIPPED behavior accurately today; this document is the target state to build toward, at which point those sections get rewritten to match.

## Build implications

- Retire remote `unbind_device`; add self-signout (device signs its own Remove).
- Add the `friend_locally_refused` set + the "reported stolen" signal (rides fleet-inbox.md); change `knows_device`/`answerable_pubkeys` to `member AND NOT refused`.
- Demote the fleet key to low-blast-radius state only (roster), since an un-removed member recovers every epoch until identity re-key.
- Custodian designation op + succession attestation + worker supersession enforcement (custodes crate).
- UI: hide locally-refused ghost members; local tombstone annotation.
