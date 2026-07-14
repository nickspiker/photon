# TODO later — parked with intent

The blinding (identity-profile stages A + B) shipped 2026-07-14; this list is what consciously waited.
Each item names its home doc so nothing needs re-deriving.

## Identity profile — stage C (the visible half; NEXT)

- Worker `profile_put`/`profile_get` at `profile/<hp>`: identity-signed envelope verified against the chain genesis pubkey (the bindreq gate's shape), opaque encrypted blob, ~4KB cap for name-only v1; hub `profile` event on put. docs/identity-profile.md ## Build state.
- Client blob v1: name sealed under a random profile key; grants = the key sealed x25519-style to each friend's pinned identity pubkey, keyed `blake3(recipient_party_id ‖ hp ‖ "grant")` (recipient-findable, graph-opaque; entry-count leak accepted v1).
- Photon: publish on name set + on friend-add (grant the newcomer, republish); fetch friends' profiles on the contact-fleet-refresh ride-along; adopt into `Contact.published_name` (field exists, renders already).
- Settings **Profile page**: name field + the load-bearing note ("your handle is your identity — you don't have to fill in ANY of this"; empty name is legal and renders as the pseudonym).
- UNTIL C LANDS: contacts render as keyed voca pseudonyms ("quietFalcon") — correct, ugly, temporary.

## Identity profile — beyond C

- **Petname editor** (contact page rename; the field + roster sync + index upsert already exist) and an optional add-time "what do you call them?" prompt.
- **Extended profile card**: first/middle/last, address, lat/lon, the whole book — per-field keys, per-contact share checkboxes, live field updates, un-share = rotate + re-grant. docs/identity-profile.md ## The profile.
- **Profile key epochs + ostracism UI**: rotate on falling-out; the friend-graph fan-out. docs/identity-profile.md ## Profile key epochs.
- **Grant-carried friendship salt** layered over the identity-DH base ingredient, for per-relationship revocability of the CLUTCH secret. docs/identity-profile.md ## The friendship secret.
- **Avatar upload re-key**: OWN avatar publishing still derives its key from the session seed (fine — we hold our own seed); when profile grants land, new avatar epochs ride field keys and the seed-derived wall retires for NEW content.
- **NFC invite card** (bearer token, auto-accept + loud review): rides the pairing-v2 NFC transport work. docs/identity-profile.md ## NFC invite card.

## Pairing / fleet (from the pairing-v2 line)

- **NFC tap transport** for device-add (HCE, phone↔phone): the candidate-set + selector machine is built; the tap delivers the candidate, touch selects. docs/pairing-v2.md ## Transports.
- **BLE transport**: lock word + proof beacon; milestone-A shadow beacon (radio path) already proven. docs/pairing-v2.md.
- **Self-departure UI** (Security page "Remove this device from fleet" stub) — `depart_device` client fn exists, unwired.
- **Ostracize-rotation UI** for a LOST device (rotate fleet key to a subset) — the withholding half until the S/friendship re-key bundle.
- Remove the two v0→v1 **flag-day supersession branches** (worker `handle_fleet_op`, client `ensure_member`) once no v0 chain can plausibly remain.
- Remove the pairing-v2 doc's retired-word-list leftovers: `first_bad_pair_word` still spell-checks pre-candidate entries (fine), but `words_to_pair_pubkey`/`pair_entry_complete` have no production callers.

## Known debris from the blinding flag-day (2026-07-14)

- All existing contacts/rosters/contact-indexes read as absent on the new build — re-add friends (both sides re-type handles), conversations restart. Old vault avatar caches orphan silently.
- `photonlog` CLI still accepts a handle argument to derive a submitter's seed — dev tooling, fine, but worth a party-id flag someday.
- `search_status`/`SearchResult` still carry the typed handle string transiently for the "added {h}" toast — in-RAM only, never at rest; could switch to the pseudonym for consistency.

## Standing parked items (pre-existing memories)

- CLUTCH offer-loss deadlock (pong-driven offer re-request), ceremony-on-UI-thread hitch, ClutchComplete retransmit storm.
- NAT relay tier gap (symmetric↔symmetric).
- Contacts search-box glow damage artifact.
- Chain-advance desync by msg 2 (salt-from-last_plaintext suspect) — NOTE: the blinding re-keyed every ceremony input, so retest this ON THE NEW BUILD before chasing it.
- Party colours → perceptual L≈50%.
- Android boot-locked session capsule (de-attest-on-restart).
- Live/self-update flow (docs/updates.md), fleet inbox beyond bind-attempt (docs/fleet-inbox.md).
