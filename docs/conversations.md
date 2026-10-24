# Conversations — one model at any size

Status: PLAN (2026-10-24). Phase 0 BUILT; Phase 1 next. Owner rule (HARD): a conversation of any size — you alone, you and one other, you and N — is treated identically: one model, one ceremony, one UI. Today's friendship/molecule split is debt being converged, never design.

## 0. The honest restatement of "CLUTCH for any size"

CLUTCH as built is strictly pairwise: `collect_clutch_eggs` (src/crypto/clutch.rs) sorts exactly two device keys and two party ids, and its identity binding is one DH between two identity keys. A literal N-party 12-primitive CLUTCH (docs/clutch.md §4.1–4.3) is O(N²) in 550 KB offers and KEM ciphertexts, needs every member online until all slots fill, reruns on every membership change, and needs a full identity mesh — docs/molecules.md §1 already rejected it.

What does scale is what the molecule path already does: a signed ROSTER, an era root MINTED once, WRAPPED to each standing device's KEM bundle — O(devices) per era, asynchronous, no mesh.

So the one model is:

- **Bond** — one per pair of people (and per pair of our own devices: siblings), never listed in the UI. Today's Contact plus the pairwise 12-primitive CLUTCH: mutual consent, identity binding, PQ breadth. It carries only control traffic (offers, joins, wraps, declines, the fleet plane, the bridge).
- **Conversation** — ONE type for N = 1, 2, or more: a stable id, a signed roster, keys (today's `FriendshipChains`), a local phase, rows. Every conversation is keyed the same way (mint a root, wrap it to each standing device's bundle), and every member past the founders enters through the same JOIN, sponsored over a bond.

"CLUTCH for any size" = per conversation, one ceremony (mint → offer → join → wrap) whose pairwise core is the sponsor↔joiner bond. The UI shows it as one ladder for every conversation.

## 1. What is already shared

The Conversation object (src/types/conversation.rs — only its `molecule: bool` splits it), `FriendshipChains` (born by `from_clutch_woven` or `from_molecule_root`), `derive_era_keys`, era routing on receive, `transmit_core` with `Vec<Route>` and `Route.party`, row storage (table = conversation id for every kind), the keys-blob codec (molecule extras gated on a flag), fleet replication of keys, the pending ledger shape (`targets`/`acked_by`), and the admission door (contacts plus token-scoped `molecule_admits`).

## 2. What is duplicated (and converges)

Key ceremony (CLUTCH vs founder mint), membership (implied pair vs Roster), conversation id/token (pair-derived vs random), key distribution (eggs vs wraps), era transitions (light ratchet Init/Resp/Nudge + heavy weave vs mint/wrap/cutover-on-ACK), admission (consent_mutual + knock vs Offer/Join/Wrap/Decline), sender attribution (`other_participant` vs fold over standing), routing, send entry, weave policy, ACK ledger, retransmit, sibling row push, history recovery (contacts only), persisted state, app-state fields, Ready rows, filters, panels (ContactPanel About/Stats/Manage vs MoleculePanel Manage/Bonds), the status/crypto line (suppressed for molecules by `Contact::molecule_view` forcing Complete), the header, and the invitations UI.

## 3. Target shape

```
Contact (person) ──owns──▶ Bond { id: BondId(sorted pids), keys, token }
                           born by pairwise CLUTCH; carries Offer/Join/Wrap/Decline, WFD cred; siblings: fleet plane, bridge
Conversation (any N) { id: ConversationId (stable), roster: Roster, keys: Option<ConversationKeys>, local: ConversationLocal, rows }
  participants = roster.standing(); token stored in keys as data
  genesis = Derived{parties}  (self-certifying: id == FriendshipId::derive(parties); every party a founder)
          | Minted{founder, nonce, sig}
  keys born by MINT, delivered by WRAP; eras by MINT/WRAP for every size
  era triggers: 256-row cadence, standing loss, a device leaving any member's fold, a stranded member (re-wrap whole)
```

Existing data maps without moving a row table: notes → `Derived{[us]}` (same id bytes), friendship → `Derived{[A,B]}` (same id bytes, existing chains inherited), molecule → `Minted` (unchanged). Siblings are bonds, never conversations.

Security properties preserved: mutual consent before key material (no wrap to a party without a Join record or Derived membership); identity binding (every roster record verified against `fold(subject)`); PQ (bonds keep 12 primitives; conversation roots ride ML-KEM-1024 + X25519 + HQC-256 and are always carried inside an already-keyed braid, never bare); forward secrecy (unchanged lanes); continuity (`derive_era_keys`); post-compromise healing (one-shot bundles: zeroize the decaps on unwrap and republish at once, or healing is one era slower than the light ratchet); revocation (fold shrink of any standing member → mint, wrap set = bundles ∩ current folds). Also kept: secrets never ride row text, lanes stamped by era tag, claim-first single completion, wraps name their bundle and are matched before decapsulation (HQC aborts on a foreign key), re-clutch-never-store, and members withhold a mint whose wrap set is short against their own roster.

## 4. Phases

**Phase 0 — groundwork. BUILT 2026-10-24.** Fold-checked roster records: every record (genesis, member, leave, vouch, bundle, title) must be signed by a device in its SUBJECT's fold (`partition_by_fold` in src/types/molecule.rs, called by `merge_snapshot_into`; the offer snapshot a joiner installs goes thru the same merge). Before this, records verified only against the device the record itself names, so a member could forge another member's leave or vouch, or plant a bundle under someone else's party and receive every later era — including the one minted to exclude them. A record whose subject's fold we do not hold yet is parked (`molecule_parked`, RAM), its fold is fetched at once (offers fetch every stranger's fold on arrival, before Join is tapped), and every fold landing re-merges it; a record signed outside a known fold is refused loudly. The mint's wrap set skips any bundle whose device is outside its owner's known fold, so a device removed from a member's fleet receives no new era. Golden test `golden_conversation_derivations` (src/types/friendship.rs) pins `FriendshipId::derive` (1 and 2 parties), `derive_conversation_token`, `MoleculeId::from_nonce` + `token`, `derive_era_keys`, and `derive_era_transcript`.

Known limits found on the way, carried forward: (a) the "a member withholds a short wrap set" tripwire that the mint transcript relies on (molecules.rs comments, §3 above) is NOT implemented — `install_molecule_wrap` installs any wrap that opens; Phase 3 builds it. (b) For a never-friended member, party ↔ handle_proof is bound only by their own member record (first verified record wins, via the sponsor's snapshot) — trust-on-first-sight thru the sponsor, because the party id derives from the handle we never learn. A bond (Phase 3) is what binds it for a friend.

**Phase 1 — one surface (UI only, no wire or storage change).** A per-frame read model `ConvView` (members, phase, crypto ladder, ring, colour, avatar) is the only place still asking "contact or roster?". One `AppState::ConversationPanel` with Bonds · Stats · Manage for every conversation, plus a Person drill-in (today's About) opened by tapping a member. One status line (`CryptoView`): each remote member's CLUTCH step, waiting for their add, joining / waiting on a wrap, catching up · era n, secured. `Contact::molecule_view` deleted. One Ready row kind; invitations are Ready rows (phase Offered) instead of cards inside a friendship. One colour rule. Invite/Leave in a pair-derived conversation is refused by data (keys not bond-independent) until Phase 3.

**Phase 2 — one model (types, storage, send/receive/ACK; wire-neutral).** `Conversation` embeds its roster and local state; `molecule: bool` and every gate on it go; `FriendshipChains` → `ConversationKeys` with a persisted token (codec v11); one `send_in_conversation`, one `routes_for_conversation`, one receive attribution (signer → party via folds over the standing set, which also commits never-friended members' frames), one ACK ledger, one retransmit pass, one sibling push keyed on the stored token, history recovery over conversations. One-shot migration at load: friends → `Derived{[us,them]}` keeping table and token; notes → `Derived{[us]}` with the self Contact deleted; molecules unchanged apart from renames; offers → invitations.

**Phase 3 — one key agreement (flag day).** Bond split (`BondId`, bond keys under their own domain; each friend re-CLUTCHes once in the background); every conversation token = `ConversationId::token`; eras by mint/wrap for every size (Init/Resp/Nudge and the heavy weave retire from conversations, bonds keep the heavy weave); any standing device may mint, ties by lowest (party, device); one-shot bundles; new pairs found `Derived{[A,B]}` at bond completion; notes minted at "New atom", the self-handle add path deleted; invitations ride bonds, so invite and Leave work at every size; strand pull + join-by-replay (molecules.md §11) so every conversation weaves.

## 5. Inventory of special cases

- **N=1:** the self-add path (input.rs, launch.rs `is_self` forcing Complete); `refresh_self_row_avatar`/`settle_self_display`/`scrub_zero_remote_rounds` (sync.rs); the zero-remote keygen guard (ceremony.rs); the panel's `is_self` arms (OwnNotes*, SelfNoCeremony, SelfNoChain, AlwaysReachableSelf); `is_self_contact` colour/alignment in render.rs; notes under the Friends filter; the `[us,us]` notes token in sibling push. Kept as the general rule evaluated on an empty set: delivered-on-durable-write, `is_reachable`/`is_keyed`, `has_remote`/`is_zero_remote`, nobody-to-push-a-blob-to.
- **N=2:** `Contact::conversation`/`remote_count`/`is_keyed`; `other_participant`; `route_for_contact`; `chain_transmit_with`; the friendship ACK arm; molecule-gated ledger fns; `dm_conversation`/`conv_of`; the SHADOW SEAM; garbage → pairwise rekey; the CLUTCH ladder bound to Contact; ContactPanel; `RowView::of_contact`; the light ratchet and heavy weave; offer cards inside a friendship; the "bring into a molecule" picker on a contact.
- **Molecule-only:** every `is_molecule()` branch in conversation.rs, status.rs, messaging.rs, sync.rs, era.rs, driver.rs, input.rs, render.rs, storage/friendship.rs, storage/molecule.rs, launch.rs, and the in-group gating of `on_molecule_signal`.
- **Siblings:** an `is_sibling` that chooses key material, party id or bond behaviour stays (siblings are bonds); one that filters people or conversations in a conversation path disappears with Contact-as-conversation. The bridge terminal moves off the conversation screen.
- **Waves** stay pairwise (`remote_standing == 1`) until N-party waves are designed — an honest capability limit, shown as such.

## 6. Owner decisions

- D1. OPEN (Nick 2026-10-24: "every member runs with every other member, sorted keys, everyone gets a deterministic same seed to init keys, then we cross keys — I swear we got it to O(N)"). No O(N) design is in docs, memory or history. docs/clutch.md §4.1–4.3 is O(N²) (every member encapsulates to every other) and its §4.3 seed derivation does not agree for N ≥ 3: each party hashes ITS OWN pairwise secrets, so Alice hashes A–B and A–C, Bob hashes A–B and B–C, and the seeds differ. Candidate O(N) shape under discussion: the STAR CLUTCH — members sorted, a deterministic leader, every member encapsulates a secret contribution to the leader (N−1), the leader combines them and encapsulates the result back to every member (N−1); N = 2 is today's cost, N = 1 is trivial.
- D2. DECIDED: Ready filters are `All · Invitations`.
- D3. DECIDED: a person's colour is keyed on the pair (you, them), so it is the same for that friend on your devices in every conversation; on someone else's screen it differs. A conversation row draws its members in their pair colours.
- D4. Bridge terminal moves to the device page (not yet asked).
- D5. DECIDED: hold Phase 1 for Phase 3 — the convergence ships together, every capability working at every size on day one. Phases are still built in order.
- D6. Add Frodo-976 to the conversation KEM set (~15 KB per bundle) for unstructured-lattice diversity (not yet asked; moot if D1 lands on the full CLUTCH primitive set).
- D7. One header format for every size ("title or member names · n") (not yet asked).
