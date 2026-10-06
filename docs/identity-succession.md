# Identity succession — continuity-proof re-pin

Status: **proposal, awaiting approval.** No code changed. Enables the Tier 2 rollover in
[fleet-identity-remediation.md](fleet-identity-remediation.md), and is a general identity
primitive (re-key, format migration, one-day out-of-band-strength identity).

## The problem it solves

A fleet's identity, to a contact, *is* the genesis hash they pinned (`pinned_genesis`,
`src/types/contact.rs:271`). Any change to the chain's genesis bytes changes that hash, so a
re-founded chain reads as a stranger (`wrong_identity`) and every contact must delete-and-re-add.
That is the only thing making a format migration (or any re-found) painful. Succession removes it:
a re-founded chain proves continuity with its predecessor, and contacts **auto-migrate their pin**.

## Why this can be secure when nothing else was

The whole identity discussion established that anything derived from the handle is forgeable
(handles are public). Succession does **not** touch the handle. Its trust anchor is a **device
key from the old chain** — a fingerprint-oracle secret, never handle-derived, the same anchor
that already gates fleet membership. An attacker who knows the handle can found a chain at the
slot but cannot produce a signature from a device the contact trusted in the old chain. That is
the exact property device-secret-gated membership already relies on; succession reuses it.

## The intuition: chain sponsors chain

Adding a device to a fleet: an existing member's device signs a consent egg admitting the new
device (`OpKind::Add`, the sponsor's egg + the joiner's consent). Succession is the same move one
level up: a device of the **old chain** signs a **continuity egg** admitting the new *chain* as
its successor. Not a phantom device — a real old device, vouching across the chain boundary.

## Record format

A `SuccessorRecord`, published once per re-found, self-contained so a contact needs no prior state
beyond their pin:

```
SuccessorRecord {
  handle_proof:      [u8; 32],        // stable across re-found (same handle → same proof)
  new_genesis_hash:  [u8; 32],        // the v2 chain's genesis hash (the pin to migrate TO)
  predecessor:       MembershipBlob,  // the FULL old (v1) chain — small; membership ops only
  continuity_eggs:   Vec<Egg>,        // one per re-founding device; see below
}
```

`predecessor` is embedded (not referenced) because re-founding **overwrites** the old chain at the
handle_proof slot — a contact can no longer fetch it. Embedding makes the record self-authenticating:
the contact folds `predecessor`, confirms it hashes to their pin, and reads its member set directly,
holding no stale state of its own.

Membership chains are small (genesis + a handful of Add/Remove/Checkpoint ops, never messages), so
the embed is cheap. The record lives at a succession slot keyed off `handle_proof`, write-gated at
the worker exactly like other fleet writes (signer must fold as a current member of the *new* chain).

## The continuity egg

Each re-founding device signs, with its device key:

```
continuity_sig = Sign_device( DOMAIN ‖ handle_proof ‖ old_genesis_hash ‖ new_genesis_hash )
```

- `DOMAIN` = a fresh tag, e.g. `PHOTON_SUCCESSION_v1`, so a succession signature can never be
  confused with a fleet-op or bindreq signature.
- `old_genesis_hash` = `predecessor.genesis_hash()`.
- Signed by **every device that is a member of BOTH the old and new chains** (the physically-same
  devices, re-founded). Multiple eggs maximise the chance a contact matches one against the old
  member set, and let succession still verify if one device has since departed.

## Contact-side auto-re-pin

On a contact refresh where the fetched chain's genesis hash ≠ `pinned_genesis`:

1. Fetch the `SuccessorRecord` for `handle_proof`. Absent → `wrong_identity` (unchanged: a genuine
   stranger re-claim or a corrupt read).
2. Fold `predecessor`; confirm `predecessor.genesis_hash() == pinned_genesis`. Mismatch →
   `wrong_identity` (this successor is not for the chain I pinned).
3. Confirm `new_genesis_hash` equals the genesis hash of the chain I actually fetched at the slot.
4. Verify at least one `continuity_egg` against a device pubkey in `predecessor`'s **folded member
   set**. That device — one I trusted as part of your old fleet — vouches for the new chain.
5. On success: migrate the pin (`pinned_genesis = new_genesis_hash`), adopt the new chain's folded
   member set, log `IDENTITY: <fp> succeeded <old>→<new>, pin migrated`. On any failure:
   `wrong_identity`, exactly as today.

A contact who **never** pinned you (a new relationship) has no old pin to migrate: they ignore the
successor and TOFU-pin the new chain directly. Succession only helps *existing* contacts; it grants
no new trust and does not change first-contact behavior.

## Monotonicity (anti-downgrade / anti-replay)

Only accept a successor whose `predecessor.genesis_hash()` equals the contact's **current** pin.
Once migrated to `G_new`, the pin is `G_new`; a replayed `G_v1→G_v2` record (old = `G_v1` ≠ current
pin) is ignored. Succession moves strictly forward; a record can never walk a contact back to an
older genesis. Replays are idempotent (they re-assert the same migration).

## Security analysis

- **Handle-knowing attacker:** can found a chain at the slot but cannot sign a continuity egg from a
  device in the old chain's member set (no device secret). Step 4 fails → `wrong_identity`. No
  auto-re-pin. This is the load-bearing property.
- **Slot takeover while you are live:** impossible — the slot is occupied and the successor write is
  member-gated; a non-member cannot publish it.
- **New contacts:** unaffected — TOFU-pin the new chain; first-founder-wins is unchanged and honest.
- **Replay / downgrade:** closed by monotonicity above.
- **Stale member set:** not relied upon — `predecessor` is embedded and self-authenticating, so the
  contact verifies from their pin alone, not from any set they happen to still hold.

## Edge cases

- **Multi-hop (pinned `G_v1`, current is `G_v3`):** the core spec is single-hop, which is all the
  Tier 2 migration (v1→v2, once) needs. General multi-hop = a contact walking a succession chain,
  each successor embedding its immediate predecessor; documented as an extension, not built now.
- **Old chain gone from the slot:** handled by embedding `predecessor` in the record.
- **Racing re-founds / two devices re-found at once:** the worker's monotonic guard on the chain
  slot (already present for fold-with-ts) resolves to one winner; the losing successor's
  `new_genesis_hash` won't match the slot's chain (step 3) and is ignored.

## The full Tier 2 rollover, using this

1. Ship **v2 dual-path** (fleet-identity-remediation.md Tier 2): new genesis drops `identity_sig`,
   keeps `identity_pubkey`; v2 clients fold both v1 and v2. No coordination.
2. Ship **succession** (this doc): the re-found flow emits a `SuccessorRecord`; contacts auto-re-pin.
3. Over a grace window, each identity re-founds under v2 at its leisure; contacts migrate silently.
4. Once every relevant chain is v2 (for a known 15-peer network, you *know*), ship the version that
   **deletes the v1-verify path**. The code shrinks; the field is gone from every live chain.

Step 2 is what makes step 3 automatic and step 4 reachable — it is the "automatic conversion,"
done as conversion-to-a-new-chain because an immutable pinned hash cannot be edited in place.

## What it does NOT provide

- It does not make identity unforgeable to someone who has never pinned you — that is still
  first-founder-wins TOFU (only a handle-independent secret would fix that; out of scope).
- It does not preserve the genesis hash — it *replaces* it and migrates the pin. There is no way to
  preserve a hash while changing the bytes it commits to.

## Test plan

- `succession_record_round_trips` — embed + eggs survive the VSF codec.
- `contact_auto_repins_on_valid_succession` — pinned `G_old`, valid record with an old-member egg →
  pin migrates to `G_new`, new member set adopted.
- `succession_rejected_without_old_member_egg` — a record whose continuity eggs are signed by a
  device NOT in the folded predecessor set → `wrong_identity`, no migration (the attacker case).
- `succession_predecessor_must_match_pin` — `predecessor.genesis_hash() != pinned_genesis` →
  rejected.
- `succession_is_monotonic` — a replayed `G_v1→G_v2` record after the contact already holds `G_v2`
  is ignored.
- `new_contact_ignores_successor_and_tofu_pins` — no prior pin → pins the new chain directly.

## Resolved decisions

1. **Separate `SuccessorRecord` at a succession slot** — not a field inside the v2 genesis. Keeps
   genesis clean; a fresh identity carries nothing.
2. **Single-hop** — covers the v1→v2 migration. Multi-hop (walk-the-chain) is a documented future
   extension, not built.
3. **No sunset** (decided 2026-10-06, below) — the v1-verify path is a permanent reader of the
   signed v1 genesis. A compile-time tripwire tried to force the question four times and was
   bumped four times; the precondition it guarded was never reachable.

## Sunset decision (2026-10-06): the v1 verify path stays

The v1 path (`verify_identity_binding`, the fold's genesis arm accepting a present `identity_sig`,
its encode/parse) folds every chain founded before the v2 cutover. Chains are immutable and
append-only, so such a chain is v1 for as long as it exists; the only route to v2 is a re-found
thru the succession re-pin flow, whose emit side has never been wired. The tripwire that lived in
`src/network/fgtw/fleet.rs` from 0.57 fired at the v70, v82, v94 and v106 deploys and was bumped
each time with the same unmet precondition — which was never about app versions: with every client
current, every chain would still be v1 (Nick's own fleet included).

Ruling: keep it. It is one extra Ed25519 verify over a typed VSF op, fail-closed and strictly
stricter than v2 — a reader of a signed binary record, not a compat text hatch. The tripwire is
deleted; the honest criterion, should anyone ever want to delete the path, is a census showing no
contact anywhere pins a v1 genesis, and that can only follow a re-found flow that does not exist.

## Deployment ordering (worker first)

The **FGTW worker** (`fgtw-bootstrap`) folds chains to gate member-only writes (Add/Checkpoint), so
it must parse and fold a v2 genesis **before any client founds one** — otherwise a v2 fleet can't
accept writes (the worker rejects the gated Add it can't fold), or the genesis publish itself is
refused. Good news: `fgtw-bootstrap/src/fleet.rs` is now a re-export of the shared crate
(`pub use fgtw::fleet::*;`) — the hand-mirrored copy is gone, so the worker gets the v2 logic by
**rebuilding against the updated `fgtw` crate**, no porting. Verified: `fgtw-bootstrap` builds clean
against these changes. **Rebuild and redeploy the worker first, then release clients** — the client
founds v2 unconditionally (`ensure_member` → `genesis_v2`), so it hard-depends on a v2 worker.

## Build order

1. **v2 dual-path** — DONE in `fgtw/src/fleet.rs`: `genesis_v2()` drops `identity_sig`, keeps
   `identity_pubkey`; encode omits the empty sig; parse discriminates by type at position 7; fold
   verifies the binding only when present. `ensure_member` founds v2. (`chain_hash` needed no change
   — blake3 over the now-empty `identity_sig` is already a no-op.) 29 fleet + 92 fgtw tests green.
   REMAINING: port the same to the worker's copy (above).
2. **Succession** (this doc): the shared-crate PRIMITIVE is DONE in `fgtw/src/fleet.rs` —
   `SuccessorRecord` (struct, `new` builder, `verify_for_pin`, `to_section`, VSF codec),
   `ContinuityEgg`, `succession_signing_bytes`, `SUCCESSION_DOMAIN`. 6 succession tests green
   (round-trip, valid re-pin, attacker-egg rejected, pin-mismatch, handle-proof-mismatch, monotonic).
   - **Worker slot — DONE** (`fgtw-bootstrap/src/lib.rs`): `handle_succession_put` (member-gated,
     mirrors `fstate_put`: device-signed envelope, signer must fold as a current fleet member,
     structural `from_vsf_bytes` parse before store, record `hp` must equal envelope `hp`) +
     `handle_succession_get` (public read), `succession_key`, dispatch on section names
     `succession`/`succession_get`, `succession/` in the data-prefix registry. `cargo check`
     (wasm32) clean.
   - **Client oracle — DONE** (`fgtw/src/client.rs`): `fetch_successor` (public read),
     `publish_successor` (device-signed envelope). Photon wrappers in
     `src/network/fgtw/fleet.rs`.
   - **Contact receive path — DONE** (`src/ui/photon_app/`): on a fold whose genesis differs from
     the pin, `protocol.rs` collects a probe → `devices.rs::spawn_successor_check` fetches the slot
     and runs `verify_for_pin` against the held pin OFF-THREAD → on a verified `Some(new_genesis)`
     the tick migrates `pinned_genesis`, clears `identity_superseded`, and re-folds. In-flight
     guard dedupes concurrent probes; a later refresh re-probes (record may publish late). An
     unverifiable record leaves the contact a stranger (no pin move).
   - **TICKET — emit side (NOT wired):** there is no re-found flow that BUILDS a `SuccessorRecord`
     and calls `publish_successor`. It is net-new (no existing identity-recreation UX to hook) and
     needs a UX decision: when/how a user declares "I re-founded this identity" (the re-founder must
     still hold ≥1 old-chain device to sign a continuity egg via `SuccessorRecord::new`). Until this
     ships, the receive path above is inert in practice — nothing publishes a record for it to find.
3. **Tripwire** — RETIRED 2026-10-06 (see "Sunset decision"): it fired four times on a precondition
   no release could meet.
4. **Sunset** — not planned. The v1 verify path is permanent unless a re-found flow one day makes
   every pinned genesis v2.
