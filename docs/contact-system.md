# Contact system — fleet-resident roster, per-contact disclosure, request-to-share

Status: SPEC (2026-07-15). Extends docs/identity-profile.md (stages A+B built; this is the concrete VSF + the request layer stage C was missing). NOT built.
Governing rule: sovereign records — the subject signs, others verify or withhold; pending expires, completed is permanent testimony; ostracism, never erasure.

## The one-paragraph model

Your contacts, your profile, and the ledger of what you've disclosed to whom all live on YOUR FLEET — replicated across your own devices, authoritative nowhere else.
fgtw.org is a fallback rendezvous only: it carries an ENCRYPTED copy of the fleet-shared blob (the existing `fstate` slot) that a fresh device or a device that can't find a peer IP pulls to bootstrap — it is never the system of record and it never holds plaintext.
Disclosure is per-field, per-contact, and opt-in: nobody reads a field of yours except across an edge you granted, and a contact who wants a field you haven't shared REQUESTS it — a signed ask your fleet surfaces, which you grant (check the box) or ignore (withhold, no record).
Recovery is the honest consequence of that: your fleet is your redundancy; lose it all with no friends and it's gone (nobody else held it, which is why nobody can leak it); lose it all with friends and you recover, edge by edge, exactly the subset you shared with each — plus the friendship itself, because they still hold your pin.

## Two enforcement layers, and what the recipient sees

Disclosure has a HARD layer and a SOFT layer, and being honest about which is which is the whole point.
- HARD (cryptographic, unforgeable): a field key gates the ciphertext, so un-sharing = rotating the key means the recipient can never decrypt a FUTURE value — you move house, re-share reaches everyone but them. This is math; no client can cheat it.
- SOFT (cooperative UI, honest-client): the plaintext a recipient already decrypted is theirs forever — no crypto un-rings that bell (they COULD have screenshotted it; probably didn't). So the honest client is asked to respect the sharer's intent at the UI, and Photon's does — it shows the disclosure state plainly instead of pretending.

Design for the honest majority, name the adversarial edge: a malicious fork can retain and re-display anything it ever saw, and the design doesn't pretend otherwise; what the shipped client guarantees is that IT behaves respectfully, which is what actually governs the 99% who run it.

The slot taxonomy is UNIVERSAL — every contact card renders the SAME standard set of slots (name, avatar, address, phone, email, birthday, SSN, … extensible) — so "not shared" looks identical for everyone and reveals nothing about whether the subject even filled a slot in.
Per slot, the recipient's client shows exactly one of three honest states:
- SHARED — the value (or empty/placeholder if the subject granted the slot but left it blank).
- NOT SHARED — no value, a plain "Alex hasn't shared their SSN with you" line, and a REQUEST affordance (§6). Rendered IDENTICALLY whether the subject never filled it, filled-but-never-granted, or declined a past request — indistinguishable by design, so a "no" leaks nothing and needs no awkward "declined" popup.
- NO LONGER SHARED — the LAST value they were given, GREYED, labelled "Alex stopped sharing this" (driven by the revoke notice §7). Not deleted, not left looking live: honestly stale.

That is the reversal from this spec's first draft (which sent "silence" on denial): the resting state is ALWAYS visible and accurate, so there is no "denied vs not-yet-seen" ambiguity to design around — a declined request simply leaves the slot reading NOT SHARED, exactly as it did before the ask.

## The field taxonomy (a VSF dictionary)

Every `field_id` is a VSF `d` (dictionary) label so it interns once and dedups across every record.
STANDARD fields have CANONICAL labels — the same label on every client and every device, so encodings agree and a field never forks into two dictionary entries.
CUSTOM fields carry a user-chosen label as its own `d` entry (interned within that user's file, not globally canonical); add as many as you want.

Always-granted slots (the card's face, never opt-in): `name` (display / preferred name) and `avatar`.

Standard optional fields — ALL default UNSHARED:

| label | field | tier |
|---|---|---|
| `first` `middle` `last` `nick` `prefix` `suffix` `maiden` `phon` | names + honorifics + pronunciation | name |
| `email` `email2` `mobile` `phone` `work_phone` `web` `alt_msg` | primary/secondary email, mobile, POTS landline, work, website, other messaging handle | reach |
| `addr` `addr_work` `geo` `tz` | postal, work address, precise lat/lon, timezone | place |
| `dob` `pronouns` `gender` `lang` `bio` | date of birth, pronouns, gender, languages, short bio | personal |
| `org` `title` | organization, job title | work |
| `ssn` `passport` `license` `tax_id` `emergency` | national id, passport, driver's license, tax id, emergency contact | sensitive |

The `sensitive` tier is a UI flag, not a crypto difference: those fields get the extra "you can't un-give this — un-share stops updates, it does not claw back" nudge before a first share, and the client won't silently fold them into a default-share set.
Custom fields ride the SAME per-field key + grant + beacon machinery as any standard field, and render on a recipient's card ONLY when shared — a recipient can neither see nor request a custom field's existence (that would leak that you keep it at all).
UI note: the universal not-shared list is long, so the card shows shared fields prominently and collapses the rest under a "request more…" affordance — standard slots are always requestable, custom ones are invisible until granted.

### 8. Default-share set (per-user config)

Section `default_share`, fleet-resident (part of your own settings) — the exception list to the unshared-by-default rule:

```
default_share:
  (repeated) auto:
    d   field_id            # a standard or custom label auto-granted to every NEW contact
```

The GLOBAL default is unshared; this list is only what a new friend gets automatically at add-time, on top of the always-granted `name`+`avatar`.
Yours = `{ first, last, email }`.
Applied when a friendship completes: iterate `default_share`, grant each field to the new contact (§4) and mark it shared (§5).
NOT retroactive — editing the list changes what FUTURE contacts receive; existing sharing is per-contact from then on (adjust an existing friend by checking/un-checking on their card).

## Storage tiers (where each thing lives)

1. Fleet-resident, authoritative — every device in your fleet holds the full set in its local vault; this is the source of truth.
2. Fleet-to-fleet sync — when your devices can reach each other (LAN, punched P2P), they reconcile directly, newest-wins by eagle-time, no server involved.
3. fgtw fallback — the sealed `fstate` blob (fleet-key AEAD) is the bootstrap copy: written on change, read by a fresh device or one that has no peer address yet; a transient relay, cleared by a nuke, decryptable only inside the fleet.
4. (Future, opt-in) friend-hosted replication — a user MAY later choose to stash an encrypted copy with a trusted friend node for extra durability; out of scope for v1, mentioned so the schema leaves room.

The disclosure ledger (tier 1 only) NEVER rides tiers 3/4 as anything a recipient could read — who you shared what with is your business; the sealed field keys a contact holds are their grant, but the map of all your grants stays fleet-internal.

## The records (full VSF)

VSF conventions used below: `hP` handle-proof (32), `ke` 32-byte key/pubkey, `ge` 64-byte Ed25519 signature, `hb` 32-byte hash, `e6` eagle-time oscillations (i64), `u` uint, `x` UTF-8 string, `d` interned dictionary label (used for every `field_id` — one copy per file, referenced by index, so a field named across profile+grants+ledger pulls and dedups clean), `ve` v'e'-encrypted value (sealed ciphertext), `a` array. Each record is a named VSF section; repeated rows use `append_multi(name, [...])`.

### 1. Contact (the pin-set — what YOU store about a friend)

Section `contact`, one row per friend (extends today's `contact_list_schema`):

```
contact:
  hP  handle_proof            # routing / addressing only
  ke  party_id                # their pinned identity pubkey — the trust anchor
  ge  avatar_pin              # 64B: AES key(32) ‖ FGTW avatar lookup hash(32); zeroes = unpinned
  x   petname                 # local override, ALWAYS wins on your screen; empty by default
  x   published_name          # adopted from their signed profile (§2); empty until granted+fetched
  ve  their_grant             # the field-key bundle THEY sealed to you (lets you read their profile); empty until they grant
  e6  added
  e6  updated
  u   flags                   # bit0 tombstone, bit1 sibling, bit2 clutch_complete
```

Names carry ZERO trust (petname → published_name → keyed voca pseudonym from `party_id`); the pinned `party_id` carries all of it.
`their_grant` is opaque to your fleet-mates in transit only in the sense that it's sealed to YOUR identity — it's yours to hold; it is synced fleet-wide (your other devices need it to render the friend too).

### 2. Profile (YOUR fields — the per-field contact card you own)

Section `profile`, identity-signed, one per fleet (yours). Every field is sealed under its OWN field key so a grant can hand out any subset.

```
profile:
  hP  handle_proof
  ke  identity_pubkey         # must match your genesis pin
  e6  version                 # bumped on any field change; monotonic
  (repeated) field:
    d   field_id              # "name", "avatar", "addr", "lat_lon", "dob", … ("name"+"avatar" are the always-granted slots)
    u   field_epoch           # bumped when this field's key rotates (un-share)
    hb  key_id                # BLAKE3(field_id ‖ field_epoch) — names the key without revealing it
    ve  value                 # v'e' ciphertext under the field key; may be empty ("" name, absent avatar are legal)
    e6  field_version         # per-field update stamp (move house once → live update to all grantees)
  ge  identity_signature      # Ed25519(identity_seed) over the canonical section bytes
```

"Required" = the SLOT (`name`, `avatar`) is always in a grant; the CONTENT may be empty — the handle is the identity, so the user need fill in nothing.
Updating a field = new `value` + bumped `field_version` under the SAME `key_id`; every contact holding that key sees it immediately.

### 3. Field-key vault (SECRET — fleet-only, never leaves except sealed in a grant)

Section `fieldkeys`, sealed under the FLEET key when it rides tier 3:

```
fieldkeys:
  (repeated) fk:
    d   field_id
    u   field_epoch
    ke  field_key             # the raw 32B symmetric key that decrypts §2's `value`
```

This is the only place raw field keys sit in the clear, and only inside your own fleet's sealed state.
Rotating a field (un-share from someone) = new `field_epoch` + new `field_key` here, re-encrypt §2's `value` under it, re-grant (§4) to the REMAINING checked contacts.

### 4. Grant (what you sealed TO a contact — their read capability)

Section `grant`, identity-signed, one per (you → contact); delivered to them, and a copy kept fleet-side for re-issue:

```
grant:
  ke  recipient_party_id      # the contact's pinned identity pubkey
  e6  issued
  (repeated) field_grant:
    d   field_id
    u   field_epoch
    ve  sealed_field_key      # §3's field_key sealed via x25519(your identity scalar → recipient identity pubkey)
  ge  identity_signature
```

The grant is exactly the bundle of checked fields' keys sealed to one identity — the friend-graph analog of the fleet-key fan-out.
Findable by the recipient, opaque to the graph: stored (tier 3 fallback) under `blake3(recipient_party_id ‖ your_hp ‖ "grant")`.

### 5. Disclosure ledger (YOUR record of what's shared with whom — tier 1 only)

Section `disclosure`, fleet-resident, the thing the Self/contact UI reads to draw the checkboxes:

```
disclosure:
  (repeated) shared:
    ke  contact_party_id
    d   field_id
    u   granted_epoch         # which field_epoch this contact currently holds (< current epoch = they have a stale/withheld value)
    e6  granted_at
    u   state                 # 0 not-shared · 1 shared(live) · 2 withheld(revoked; they keep last value as testimony) · 3 request-pending
```

state==2 is the "permanent testimony" case: on un-share you rotate the key and stop re-granting; the contact keeps the last value they were given and never sees another — the ledger remembers you cut it off, and their honest client shows that last value GREYED as NO LONGER SHARED (via the revoke notice §7), never hidden or left looking current.
state==3 is set by an inbound request (§6) awaiting your decision.

### 6. Field request (a contact asks for a field you HAVEN'T shared)

Section `field_request`, requester-signed, delivered over the friendship channel (P2P; fgtw relay `request/<your_hp>` as fallback):

```
field_request:
  ke  requester_party_id      # the contact asking — verified against your pinned contact
  (repeated) want:
    d   field_id
  e6  requested_at
  x   note                    # optional human line ("can I get your address for the package?")
  ge  requester_signature     # Ed25519 by their identity — proves it's really them, not the relay
```

Your fleet verifies the signature against the pinned contact, flips the named fields to state==3 (request-pending) in the ledger, and surfaces ONE notice ("Alex requests: address").
Grant = check the box → seal the field key into their grant (§4), flip state 3→1, done.
Deny = ignore or dismiss → the field returns to its resting NOT SHARED state on their side; no "declined" message is needed or sent, because what they already see ("Alex hasn't shared their SSN") is honest either way — the ask just didn't change it.
Anti-nag: a duplicate request for a field already pending/withheld is coalesced, not re-notified.

### 7. Revoke notice (un-share, told honestly)

Section `revoke`, identity-signed, sender → recipient (P2P; fgtw relay fallback):

```
revoke:
  ke  recipient_party_id
  (repeated) dropped:
    d   field_id
    u   at_epoch            # the field_epoch after which they receive nothing
  e6  revoked_at
  ge  identity_signature
```

Carries NO key — that's the whole point; it's the fact, not a capability.
The recipient's honest client greys the last value it holds for each dropped field and labels it NO LONGER SHARED; the crypto (§3 rotation) has already guaranteed no future value can reach them, so the notice only drives the UI truth.
Still "ostracism, never erasure": nothing is clawed back, the boundary is just made visible instead of silently freezing a stale value that looks live.

## Protocols (the verbs)

- Share field F with contact C: seal F's current key into C's grant (§4), append/patch §5 `shared` to state 1 at the current epoch, resync tiers 1→2→3.
- Un-share field F from C: rotate F (§3 new epoch + key), re-encrypt §2 value, re-issue grants to all OTHER live holders, set C's §5 state to 2 (withheld), and send C a revoke notice (§7). C keeps its last value forever (greyed, labelled NO LONGER SHARED), gets no update.
- Update field F (e.g. new email): write new §2 `value` + `field_version` under the SAME key, then BEACON it (below) to every live grantee of F; each decrypts with the key it already holds — no re-grant. Move your email once and everyone you share it with has the new one.
- Request field F (contact side): sign + send §6; on grant, adopt the sealed key and read F.
- Sync: reconcile §1/§2/§4/§5 across your fleet newest-wins by eagle-time (tier 2 direct when reachable, tier 3 sealed blob otherwise); §5 never leaves the fleet legible.
- First-met still discloses first: a friend request carries the requester's grant (identity-profile §Grant paths), so the recipient renders who's knocking while trusting nothing; acceptance reciprocates.

## The grant index + the profile beacon (push, not poll)

The disclosure ledger (§5) IS your grant list, and it answers the two questions you need, from the same rows indexed two ways (the per-field view is derived, not a second store):
- per-CONTACT — "what does Alice hold?" — draw her card, know what you could un-share.
- per-FIELD — "who holds my email?" — the set of contacts with that field at state==1; this is the PUSH LIST for that field.

When a field changes you do NOT wait for grantees to poll — you beacon:
1. Write the new field version into your canonical profile (§2), fleet-resident + the fgtw fallback copy at `profile/<hp>`.
2. Fan a lightweight "profile updated, field F, v.N" notice to every contact on F's push list — P2P to each grantee's reachable fleet devices (tier 2), the fgtw hub "profile" event as the fallback wake for the offline ones (tier 3).
3. Each notified grantee's fleet pulls the new §2 and decrypts F with the key it already holds; the offline ones catch up on next connect (the fgtw copy is waiting for them).

This is the friend-graph analog of the fleet-key fan-out — the same fan-out-to-a-known-set machinery, one level up: the fleet key fans to your member DEVICES, profile fields fan to your granted FRIEND identities.
A contact you don't share F with is never on F's list, never beaconed, and never learns it changed.
Un-share is the same machine in reverse: drop the contact from F's push list (§5 → state 2) and send the revoke notice (§7) in place of the update.

## Recovery semantics (the tradeoff, made precise)

- One device dies → non-event: the rest of your fleet holds §1–§5.
- Whole fleet lost, no friends → total loss of §1–§5. Intentional: nobody else held it, so nobody can leak it or restore it. You keep only your identity (deterministic from the handle) and start empty.
- Whole fleet lost, friends exist → edge-by-edge recovery: each friend who still holds your pin re-recognizes you on reconnect; that friendship comes back; your MESSAGES restore from either side; your PROFILE fields restore only the subset you had granted that friend (they hold your last-granted values as testimony). You do NOT get a roster of your OTHER friends from anyone (that would leak your social graph) and you do NOT recover §3 field keys or un-shared fields.
- Custodians (opt-in, future — docs total-loss-recovery): a designated friend-quorum can authorize a fuller supersession recovery; "no friends, no custodians" stays total loss, as it must.

## Build stages

- C1 — profile record + field-key vault + own-profile publish (name+avatar slots), fleet-resident with fstate ride-along; the Self/Profile page front-end (name box, "you don't have to fill in ANY of this" note, empty legal).
- C2 — grants + per-contact checkboxes + adopt-into-`published_name` on contact refresh; the disclosure ledger (§5) and its UI; un-share = rotate.
- C3 — the request flow (§6): inbound notice, one-tap grant, coalescing; fgtw `request/<hp>` relay fallback + P2P delivery.
- C4 — (future) friend-hosted encrypted replication tier; custodian recovery.

Flag-day: rides the same re-CLUTCH flag-day as identity-profile stage C (party-id re-keying); old rows read as absent.
```
