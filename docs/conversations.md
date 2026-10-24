# Conversations — one model at any size

Status: PLAN (2026-10-24). Phase 0 BUILT (main); Phase 1 BUILT on branch `conversations`; Phase 2 next. Owner rule (HARD): a conversation of any size — you alone, you and one other, you and N — is treated identically: one model, one ceremony, one UI. Today's friendship/molecule split is debt being converged, never design.

## 0. One ceremony at any size — the key rides the bonds (distilled 2026-10-24, Nick + Claude)

**The insight (Nick):** before any conversation exists, every pair of friends already holds a secure channel — adding a friend runs the full pairwise CLUTCH (mutual consent, identity binding, all 12 primitives, braided per-message forward secrecy). That channel is the BOND. A conversation of any size never needs a key exchange of its own: its key travels over bonds that already exist.

**Why not the documented N-party CLUTCH.** docs/clutch.md §4.1–4.3 has every member encapsulate to every other member — O(N²), everyone online until every slot fills, rerun on every membership change. Its §4.3 seed also does not agree for N ≥ 3: each party hashes only the pairwise secrets IT holds (Alice: A–B, A–C; Bob: A–B, B–C), so the seeds differ. It is superseded by the ceremony below.

**Pieces.**

- **Bond** — one per pair of people (siblings: one per pair of our own devices, today's fleet weave). Born when a friend is added (today's consent gate + pairwise CLUTCH), never listed as a conversation, reaches every device of both fleets (per-device lanes). It carries control traffic only: invitations, joins, declines, era deliveries, the fleet plane, the bridge.
- **Conversation** — ONE type for N = 1, 2, or more: a stable random id, a signed roster, keys (today's `FriendshipChains`), rows. Adding a friend also founds a two-member conversation with them (the "required molecule" — listed, and it grows in place like any other, because the bond under it is separate).
- **Bond graph of a conversation** — nodes = its standing members, edges = bonds between members. Connected by construction: every member past the founders joined thru a sponsor it is bonded to.

**The ceremony (any N), one per era:**

1. **Mint.** Any standing member mints when the conversation needs a new era (founding, a join, a departure, a device leaving a member's fleet, the row cadence). Ties between simultaneous mints resolve to the lowest sorted (party, device). The next era derives exactly as today: `derive_era_keys(id, era, old_root, old_hk, fresh, transcript)` — everyone holding era e plus `fresh` lands on the same seed, so the lanes init identically ("deterministic same seed").
2. **Announce.** The minter signs an era record: (conversation id, era index, the sorted standing set it minted for, a commitment to `fresh`). The signature verifies against the minter's fold (Phase 0's resolver).
3. **Deliver over bonds.** `fresh` (or the era whole, for a joiner who never held e) goes to each member along a spanning tree of the bond graph rooted at the minter: N−1 deliveries, each one small row over an existing braid. A member that receives forwards to its children in the same tree (computed identically by everyone from the sorted roster and the bond edges they can see). Every hop is the full 12-primitive bond; no KEM bundle, no extra key material.
4. **Check and cut over.** A member verifies the era record's signature, checks the standing set against its own roster (a short set is WITHHELD — the exclusion tripwire, built here for real), derives, and cuts over on the first frame tagged with the new era or its own next send (unchanged).

**Cost.** N = 1: nothing to deliver. N = 2: one delivery over the pair's bond. N members: N−1 deliveries + the forwards along the tree — O(N), asynchronous (an offline member's subtree catches up when it wakes; nobody waits for everyone).

**What this deletes.** KEM bundles in the roster, `wrap_to_bundle`/`unwrap_from_bundle` for conversations, the bundle records and their forgery class (Phase 0's worst case), the 3-KEM conversation set — every conversation key is protected by the full 12-primitive bonds.

**Departure and bridges.** Leaving is self-only (no boot). The next era is delivered only over bonds among the remaining members, so the leaver never receives it; traffic between two remaining members rides their own bond, which the leaver is not an endpoint of. If the leaver was the only bond between two parts of the graph, the parts need a bond: two standing members who are not friends run a MEMBER BOND — the same pairwise CLUTCH, its consent being that both stand in the conversation (one-time, ~550 KB). The era waits for that bond only in the part it reaches.

**Devices.** A bond reaches every device of both fleets, so delivery to a member is delivery to their fleet. A device leaving a member's fleet re-keys that member's bonds (heavy weave, today) and then triggers a conversation mint, which reaches the remaining devices only.

**Settled (Nick 2026-10-24).** No contributory entropy: members send nothing toward the minter and nobody waits on anyone — the bonds are strong enough to carry the era. The minter's own `fresh` stays, because it is what locks a leaver out: an era derived from era e alone would be derivable by anyone who held e. The minter also delivers directly to every member it is bonded with that has not ACKed, so a slow or dropping forwarder never strands anyone.

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
  keys born by MINT, delivered over BONDS along the member bond graph (§0); eras the same for every size
  era triggers: 256-row cadence, a join, standing loss, a device leaving any member's fold, a stranded member (re-deliver the era whole)
```

Existing data maps without moving a row table: notes → `Derived{[us]}` (same id bytes), friendship → `Derived{[A,B]}` (same id bytes, existing chains inherited), molecule → `Minted` (unchanged). Siblings are bonds, never conversations.

Security properties preserved: mutual consent before key material (an era is delivered only to a standing member — a Join record or Derived membership — and only over a bond, which itself needed mutual consent); identity binding (every roster record verified against `fold(subject)`; era records signed by the minter's fold); PQ (every conversation key rides the full 12-primitive bonds — stronger than the 3-KEM bundles it replaces); forward secrecy (bond braids advance per message; conversation lanes unchanged); continuity (`derive_era_keys`); post-compromise healing (a compromised bond heals by its own light ratchet / heavy weave, and the next conversation era rides the healed bond); revocation (fold shrink of any standing member → that member's bonds re-key, then a conversation mint). Also kept: secrets never ride row text, lanes stamped by era tag, claim-first single completion, re-clutch-never-store, and members withhold an era whose standing set is short against their own roster.

## 4. Phases

**Phase 0 — groundwork. BUILT 2026-10-24.** Fold-checked roster records: every record (genesis, member, leave, vouch, bundle, title) must be signed by a device in its SUBJECT's fold (`partition_by_fold` in src/types/molecule.rs, called by `merge_snapshot_into`; the offer snapshot a joiner installs goes thru the same merge). Before this, records verified only against the device the record itself names, so a member could forge another member's leave or vouch, or plant a bundle under someone else's party and receive every later era — including the one minted to exclude them. A record whose subject's fold we do not hold yet is parked (`molecule_parked`, RAM), its fold is fetched at once (offers fetch every stranger's fold on arrival, before Join is tapped), and every fold landing re-merges it; a record signed outside a known fold is refused loudly. The mint's wrap set skips any bundle whose device is outside its owner's known fold, so a device removed from a member's fleet receives no new era. Golden test `golden_conversation_derivations` (src/types/friendship.rs) pins `FriendshipId::derive` (1 and 2 parties), `derive_conversation_token`, `MoleculeId::from_nonce` + `token`, `derive_era_keys`, and `derive_era_transcript`.

Known limits found on the way, carried forward: (a) the "a member withholds a short wrap set" tripwire that the mint transcript relies on (molecules.rs comments, §3 above) is NOT implemented — `install_molecule_wrap` installs any wrap that opens; Phase 3 builds it. (b) For a never-friended member, party ↔ handle_proof is bound only by their own member record (first verified record wins, via the sponsor's snapshot) — trust-on-first-sight thru the sponsor, because the party id derives from the handle we never learn. A bond (Phase 3) is what binds it for a friend.

**Phase 1 — one surface. BUILT on branch `conversations` 2026-10-24 (ships with Phases 2–3, D5).** One `AppState::ConversationPanel(ConversationPage)` for every conversation: Bonds · In numbers · Manage on the rail, and a Person page drilled into from any Bonds member row (avatar, what they share, identity, pseudonym, Boot). The behaviour lives in src/ui/photon_app/conv_panel.rs; the render arm reads a merged view of the two data sources. Clear works in every conversation; Boot works on any person from any conversation; the "bring into a molecule" picker is gone (Bonds on the target conversation replaces it). One status line under the header at every size: a pair's CLUTCH step until woven, a roster-backed conversation's binding / catching up / you left until it stands; the phase and count left the title. Ready filters are All · New atom · Invitations (D2). A person's colour is keyed on (them, you) everywhere, and a conversation's pie is each member's own gradient (D3). Still split until Phase 2 makes them trivial: the Ready row kinds (`ReadyRow::{Contact, Group}` behind one `RowView`), invitations as Ready rows (today the Invitations filter lists the sponsors, the offer card rides the sponsor's conversation), Mute and the shared title for pair conversations, and inviting into a pair conversation (needs the Phase 3 key split).

**Phase 2 — one model (types, storage, send/receive/ACK; wire-neutral).** `Conversation` embeds its roster and local state; `molecule: bool` and every gate on it go; `FriendshipChains` → `ConversationKeys` with a persisted token (codec v11); one `send_in_conversation`, one `routes_for_conversation`, one receive attribution (signer → party via folds over the standing set, which also commits never-friended members' frames), one ACK ledger, one retransmit pass, one sibling push keyed on the stored token, history recovery over conversations. One-shot migration at load: friends → `Derived{[us,them]}` keeping table and token; notes → `Derived{[us]}` with the self Contact deleted; molecules unchanged apart from renames; offers → invitations.

**Phase 3 — one key agreement (flag day).** The ceremony of §0. Bond split: `BondId`, bond keys under their own domain, each existing friendship's chains become its bond (re-CLUTCH once in the background only if a clean split needs fresh keys); adding a friend runs the bond CLUTCH and founds the pair's two-member conversation, whose era 0 is delivered over that bond. Every conversation token = `ConversationId::token`. Eras for every size by mint → signed era record → delivery over the member bond graph → check (the short-set tripwire) → cutover; Init/Resp/Nudge and the heavy weave retire from conversations (bonds keep both). Roster KEM bundles, `wrap_to_bundle`/`unwrap_from_bundle` for conversations, and bundle records are deleted. Member bonds for graph bridges. Notes are founded at "New atom"; the self-handle add path is deleted. Invitations, joins and declines ride bonds, so invite and Leave work at every size. Strand pull + join-by-replay (molecules.md §11) so every conversation weaves.

## 5. Inventory of special cases

- **N=1:** the self-add path (input.rs, launch.rs `is_self` forcing Complete); `refresh_self_row_avatar`/`settle_self_display`/`scrub_zero_remote_rounds` (sync.rs); the zero-remote keygen guard (ceremony.rs); the panel's `is_self` arms (OwnNotes*, SelfNoCeremony, SelfNoChain, AlwaysReachableSelf); `is_self_contact` colour/alignment in render.rs; notes under the Friends filter; the `[us,us]` notes token in sibling push. Kept as the general rule evaluated on an empty set: delivered-on-durable-write, `is_reachable`/`is_keyed`, `has_remote`/`is_zero_remote`, nobody-to-push-a-blob-to.
- **N=2:** `Contact::conversation`/`remote_count`/`is_keyed`; `other_participant`; `route_for_contact`; `chain_transmit_with`; the friendship ACK arm; molecule-gated ledger fns; `dm_conversation`/`conv_of`; the SHADOW SEAM; garbage → pairwise rekey; the CLUTCH ladder bound to Contact; ContactPanel; `RowView::of_contact`; the light ratchet and heavy weave; offer cards inside a friendship; the "bring into a molecule" picker on a contact.
- **Molecule-only:** every `is_molecule()` branch in conversation.rs, status.rs, messaging.rs, sync.rs, era.rs, driver.rs, input.rs, render.rs, storage/friendship.rs, storage/molecule.rs, launch.rs, and the in-group gating of `on_molecule_signal`.
- **Siblings:** an `is_sibling` that chooses key material, party id or bond behaviour stays (siblings are bonds); one that filters people or conversations in a conversation path disappears with Contact-as-conversation. The bridge terminal moves off the conversation screen.
- **Waves** stay pairwise (`remote_standing == 1`) until N-party waves are designed — an honest capability limit, shown as such.

## 6. Owner decisions

- D1. DECIDED (2026-10-24): the key rides the bonds (§0) — no per-conversation key exchange, O(N) per era; no contributory entropy; the minter delivers directly to anyone it is bonded with that has not ACKed.
- D2. DECIDED: Ready filters are `All · Invitations`.
- D3. DECIDED: a person's colour is keyed on the pair (you, them), so it is the same for that friend on your devices in every conversation; on someone else's screen it differs. A conversation row draws its members in their pair colours.
- D4. Bridge terminal moves to the device page (not yet asked).
- D5. DECIDED: hold Phase 1 for Phase 3 — the convergence ships together, every capability working at every size on day one. Phases are still built in order.
- D6. Moot: with the key riding the 12-primitive bonds there is no separate conversation KEM set.
- D7. One header format for every size ("title or member names · n") (not yet asked).

## 7. Phases 2–3 together: the split (2026-10-24, Nick: "I'd build all of it")

Phases 2 and 3 ship as one migration, so there is one flag day and no intermediate model on disk.

**An existing friendship splits in two:**

- **Its conversation keeps everything it has.** The rows table stays `friendship_id`, the wire token stays `derive_conversation_token([us, them])` (now stored as data in the keys blob, not re-derived), and the current chains stay the conversation's keys — history, lanes and in-flight rows are untouched, and an unupgraded peer keeps chatting. It gains a roster whose genesis is `Derived{[us, them]}`: self-certifying (`id == FriendshipId::derive(parties)`), every party a founder, no signature needed.
- **Its bond is new.** Both sides derive the bond keys from the conversation's era at migration — `root_B = KDF("PHOTON_BOND_v1" ‖ root_conv)`, the same for the history key — under a new id `BondId::derive(sorted pids)` and a new token; no network round trip, both sides land on the same bond independently. CLUTCH, the light ratchet (Init/Resp/Nudge), heavy weaves, the repair table and the stranded heal move to the bond; a new friendship's CLUTCH produces bond keys directly and then founds the pair's two-member conversation (era 0 delivered over the bond).
- **The rule that keeps the bond secret:** anyone holding the era a bond was derived from could compute the bond, so a conversation NEVER hands out an era at or below its `bond_split_era`. A join into such a conversation always mints a fresh era first (whatever its history policy), and the conversation's first era trigger after migration retires the split era for good.

**Mixed versions.** Chat keeps working across the flag day because the conversation's token and lanes do not change. Only a mint needs the peer upgraded: a conversation mints only when every member it must deliver to has been heard on its bond (the first frame on the bond token is the edge); until then it stays on its current era. Old clients ignore control rows on a token they do not know; new clients drop Init/Resp on conversations with a log line.

**Molecules** keep their ids and rosters; their control traffic (offers, joins, wraps, declines) moves from the friendship chains to the bond, and their era deliveries move from KEM bundles to the bond graph (§0), after which bundle records are deleted.

**Notes** (the self contact) become a one-member conversation `Derived{[us]}` with the same rows table; the self Contact row is deleted.

**Build order (2026-10-24, from the inventory, docs/conversations-inventory.md).** Flipping pair conversations onto the roster path first would strand their probe / era / CLUTCH traffic, which is bond business, so the bond comes first:

- A. Bonds exist: `Contact.bond_id`, split from the pair's chains at load (`bond_split_of`, `bond_split_era` recorded on the conversation's chains), persisted; each bond has a hidden Conversation (id = bond id, never listed) that holds its control rows for re-ACK durability, exactly as a pair conversation holds its hidden rows today.
- B. A bond transmit path (the pairwise route + lanes, on the bond's chains and token); frames on a bond token commit thru the pairwise receive path and carry control rows only.
- C. Molecule invites, joins, join wraps and declines ride the bond; an invitation becomes its own Ready row (phase Offered) instead of a card inside the sponsor's conversation.
- D. CLUTCH completion, the light ratchet, heavy weaves, probes and the woven seal operate on the bond.
- E. Pair conversations flip to the roster path (`Derived` genesis, molecule flag), the receive / ACK / history / unread parity gaps close, and every conversation's eras are minted and delivered over bonds.
- F. KEM bundles and their records are deleted; docs rewritten (clutch.md, molecules.md, lanes.md, braid.md).
