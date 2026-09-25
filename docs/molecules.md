# Atoms and molecules — a conversation you found alone, bound into one you share

**Vocabulary (Nick, 2026-09-15).** You can only ever create an **atom**: a conversation with you alone in it, titled or not — your notes are one, and most of yours may stay that way. **Binding** a second person in makes it a **molecule**: the same object under a different count, the way a lone hydrogen is an atom and H₂ is the molecule. A **bond** is one person's standing in it. A friendship is a distinct thing, fixed at two by CLUTCH; atoms and molecules are the mutable-set thing, and the contacts screen filters among friends, atoms and molecules. Below, "molecule" is used for the object whatever its count, and the code says `Molecule*` / `Bond*` / `found_atom` accordingly. The word "group" was retired the same day.

**Status:** PHASE 1 BUILT 2026-09-15 (steps 1–6, 8–10 of the plan; step 7's live strand pull and the never-friended receive commit are the open items — see §11). Originally a PROPOSAL of 2026-09-14. Written against the shipped substrate: per-device lanes (docs/lanes.md), the era ratchet (`crypto/era.rs`), the fleet-key fan-out shape (docs/fleet-key.md), fleet-holds-history (docs/durability.md), and the participant-set `Conversation` (`types/conversation.rs`). lanes.md promised "Phase C — same lanes off a molecule root"; this is that phase, spelled out. The word "molecules" in docs/global-vault.md means device molecules for settings linkage and is unrelated.

## 0. The one-paragraph model

A molecule is a conversation with a mutable member set and its own root secret.
There is no molecule ceremony: the founder mints the root, and every later member receives it from a sponsor over the pairwise braid the two of them already hold.
Two members who never CLUTCHed each other share only the molecule root, and that is enough: every device of every member derives its lane from the root and a label, exactly as a friendship does today (lanes.md), so receive-anywhere, writer discipline, checkpoints and fleet replication carry over verbatim.
Membership is sovereign records: a member's standing in the molecule is its own signed record, others verify or withhold; joining is posting that record, leaving is the leaver's signed request plus one survivor's countersign, and the countersign mints the next era so the leaver holds no future key.
Expulsion is not a verb; the molecule verb is "not in the next key", and the remedy for an unwanted member is a fork (a new molecule without them).
No molecule state ever touches infrastructure: the molecule holds its own history, and the fleet plane replicates each member's copy across their devices as it does for any conversation.

## 1. Why this shape and not the other two

**Not the N-party CLUTCH.** clutch.md §3.4/§4.1 already sketch a slot ceremony for N handle hashes, and `PartySlot` is N-party in the type. It should stay unused for molecules: every member's fleet must complete the ceremony before anyone can speak (a dormant fleet blocks the whole molecule), each member posts a 550 KB offer to N−1 peers, and every membership change re-runs all of it. It also demands that every member know every other member's handle, which is the very thing molecules exist to avoid.

**Not pairwise fan-out** (encrypt N−1 times, one per friendship braid). Simplest crypto, but it requires a full mesh of friendships: you could not add your cousin to the family molecule unless your sibling had CLUTCHed with them too. The acid test for any molecule design here is "I add my contact; nobody else needs to know them", and pairwise fan-out fails it. It also advances N−1 lanes per send and makes delivery N separate stories.

**The shared root** is the sender-keys shape (Signal molecules, MLS), and photon already speaks it: the fleet key is a shared secret fanned out per member and re-minted on shrink (fleet-key.md); lanes are a shared root plus a public label; eras are a two-phase root ratchet with retired-era stragglers. Molecules reuse all three with a different root.

## 2. Identity

- **Molecule id.** `molecule_id = blake3("PHOTON_GROUP_v1" ‖ genesis_nonce)`, 32 random bytes minted by the founder. It is deliberately NOT participant-derived: `FriendshipId::derive` keys a conversation on its participant set, which is immutable by construction, and a molecule's set is not. `Conversation` gains a stable-id form whose participant vec is mutable behind that id; everything downstream (rārangi table, chains blob, conversation state) keys on the id bytes as today.
- **Party id.** A member's party id is the same opaque 32-byte pinned identity pubkey friends use in `Contact.handle_hash`. A member you never friended: you learn their party id, their `handle_proof`, and (by folding the public membership chain under that proof, as `Contact` already does) their current device set. You never learn their handle; handles are keys and stay grants-only (docs/identity-profile.md).
- **Names.** A member grants the molecule its published name and avatar pin as part of its member record (the pong's always-granted name slot, scoped to the molecule). Petnames stay local. Names carry zero trust, the pinned key does.
- **Molecule token.** `molecule_token = blake3_derive("photon.molecule.token.v1", molecule_id)`, the routing tag on every molecule frame (the `conversation_token` role). Only members hold `molecule_id`, so only members can mint or match it. It is stable across eras; the era tag rides beside it as it does for friendships.
- **Transport trust, scoped.** Today every inbound frame is contact-gated (`knows_device`, `answerable_pubkeys`). A molecule adds a second trust source: a `MoleculePeer` (party id → folded devices) beside `Contact`, consulted ONLY for frames carrying that molecule's token. A non-friend member's pings still drop; presence outside the molecule is not granted by membership.

## 3. Keys

**Root.** Each era has `molecule_root` and `molecule_history_key`, both 32 bytes; `era_lineage` is the one-way image of the era-0 root, `era_index` monotonic, exactly the friendship era model.

**Lanes.** `lane = expand(molecule_root ‖ "PHOTON_LANE_v" ‖ [1] ‖ label)`, lanes.md verbatim with the molecule root as the seed. Each of my devices mints its own label at its first send in the molecule; any receiver derives the lane from the label alone. One writer per lane, so no fork; checkpoints replicate across my fleet on the same chain-replication plane, keyed by `molecule_id`.

**Frames.** The chat frame's `msg` section gains `from` (the sender's party id; in a friendship it is implicit, in a molecule it is attribution) beside the era tag. The outer Ed25519 signature is verified against the sender's folded devices from the roster. One ciphertext per row, sent to every member's active device over the same LAN → WAN → relay ladder friendships use; each member's fleet replicates inward from whichever device received it.

**The weave.** A sender weaves up to two rows it has RECEIVED, from any member (braid §6). Weave references become `(author party id, eagle_time)`, because eagle times are unique per device, not per molecule. A receiver that holds the frame but not a woven strand cannot derive the link, so it parks the frame in the gap buffer (already keyed on a missing predecessor; this adds "missing strand") and PULLS the strand from the frame's sender, who necessarily holds it: `strand_pull`/`strand_serve`, one row sealed under the molecule history key with the history-page codec. Weave references point strictly backwards in causality, so the pull graph is a DAG and never deadlocks; it is edge-driven (on the parked frame) and never timered. This keeps braid §1.4's confluence promise in molecules: any delivery order converges.

**ACKs and the sender's advance.** Receivers advance on receipt as today. The sender's lane advances on the first ACK from any member (the advance is a pure function of the row, so which member ACKed is irrelevant); the pending entry keeps retransmitting to the members that have not ACKed, and re-serve covers the rest. `delivered` on a row becomes the set of party ids that ACKed it.

**Eras.** Same three states (current, retired read-only for stragglers, pending two-phase). Next-era secrets come from the existing `derive_era_keys(molecule_id, idx, old_root, old_hk, fresh, transcript)`. What changes is how `fresh` reaches N fleets that share no pairwise channel: every device publishes, in its member record, a molecule-scoped hybrid KEM bundle (ML-KEM-1024 + X25519 + HQC-256, `era_keygen` as it exists) whose decapsulation keys live in the molecule chains blob, the same custody class as the 16 KB of lane links beside them, and which the device replaces on its first frame of every new era. The minter encapsulates `fresh` to every surviving device's bundle and posts a `GroupEra` control row on the OLD era carrying the wraps (the PFO1 blob shape, KEM in place of ECDH). A device that was offline reads the row from history re-serve; the retired era stays readable for exactly that. Triggers: SHRINK (mandatory, see §4), the 256-row cadence (any member may propose), never a heavy weave (there is no CLUTCH to weave). Two proposers for the same index resolve by lowest minter party id; the loser drops its pending, no owner election (clutch.md §10.1 ruling). Stated honestly: a leaked chains blob leaks that device's decapsulation key, which opens the next era's wrap to it, until its bundle rotates at that era.

## 4. Membership — the roster as sovereign records

The roster is a set of signed records, merged by union with subject-signed newest-wins per party. It rides inside the molecule as hidden control rows (`RowControl::Molecule`, typed like the era rows), so it is history, re-servable, and fleet-replicated with no new plane.

- **Genesis** — founder-signed: `molecule_id`, lineage, founder party id, genesis eagle time, and the newcomer-history policy (§8 D5).
- **Member** — signed by one of the member's own devices: party id, `handle_proof`, molecule name grant, avatar pin, per-device KEM bundles, `joined_osc`, sponsor party id. Everyone folds the member's devices from the public membership chain under the proof; no device list is trusted from the record itself.
- **Invite** — the sponsor's pending record, EXPIRING, sent PAIRWISE over the sponsor↔invitee friendship braid as a control row: `molecule_id`, lineage, the current era's root and history key, and a roster snapshot so the invitee sees who is in it before consenting. The secrets ride TYPED package fields (`gei`/`groot`/`ghk`/`glin`), consumed at ingress and never written to the row: the row text persists, replicates to siblings and re-serves as history, so a secret in it would outlive its era and defeat the retired-era zeroize (fixed 2026-09-15, before any molecule was ever minted). The text is the bare kind marker. The invitee's consent IS its Member record, posted into the molecule as its first frame. Any member may sponsor any contact of theirs; a rejected or expired invite leaves no record.
- **Leave** — the leaver's signed request. The first survivor to observe it countersigns and mints the next era, wrapping to every device except the leaver's; a second countersigner loses on minter id and adopts. Departure therefore always costs one mint, exactly as a fleet shrink does.
- **No expulsion.** Consistent with docs/pairing-v2.md: verbs of self (request, consent, resign) and verbs of molecule (verify, include in the next key, exclude from the next key). "Exclude from the next key" without the subject's request is a fork; v1 does not offer it as a button. The remedy is a local mute and, if needed, a new molecule.
- **Ostracism, never erasure.** A left member's records stay as testimony; its rows stay in history.

## 5. The fleet plane, unchanged

My devices see a molecule as one more conversation: chains blob (lanes, eras, KEM decaps) at `vault_key("chains", molecule_id)`, rows in the rārangi table `molecule_id`, the roster at `vault_key("molecule", molecule_id)`. Checkpoints, sibling row pushes, history pages, anti-entropy digests and the delivery ladder apply as they stand. Rows gain one additive column, `author: PartyId` (absent on old rows = derived from `is_outgoing`). No molecule slot on FGTW, no worker change: durability.md's option B by analogy, molecule-holds-history.

## 6. UI

- Contacts list: a molecule row, avatar = a composite of member gradients (no orb of its own), unread ring as today.
- Conversation: author name and `party_colour` of the author's party id on each row; the delivered ring fills by the fraction of members that ACKed. Header lists members.
- New molecule: pick from your contacts; a molecule page with members, Add (your contacts only), Leave.
- Honest surfaces: "history before you joined isn't shared" when D5 is off; "N members haven't received this yet" is the delivered fraction, nothing timered.
- Waves in molecules are out of scope for v1 (waves.md is pairwise).

## 7. Concrete additions

- `types/molecule.rs`: `MoleculeId`, roster records + merge, the `MoleculeSignal` control-row kinds (typed as `RowControl::Molecule`; payloads ride the package's `g*` fields).
- `Conversation`: stable-id form with a mutable participant set; `ChatMessage.author`.
- Frame: `from` party id; weave refs `(author, eagle_time)`; `strand_pull`/`strand_serve` on the history-page codec.
- Chains blob (v10 since 2026-09-25, one section per record): molecule flag, per-device KEM decapsulation keys as `kem` sections, lanes off the molecule root.
- `MoleculePeer` trust source consulted for molecule-token frames only.
- Storage: `vault_key("molecule", molecule_id)`; rārangi table = `molecule_id`.

## 8. Decisions for Nick, with a recommendation each

- **D1 Shape.** Shared root delivered pairwise (this doc) vs N-party CLUTCH. Recommend the shared root, §1.
- **D2 Id.** Stable random `molecule_id` vs participant-derived. Recommend stable; membership moves, the conversation must not.
- **D3 Adds.** Any member sponsors any contact of theirs, no vote. Recommend yes for v1; a consent gate on adds can layer later as a genesis flag.
- **D4 Expulsion.** None; fork is the remedy. Recommend yes, it is the doctrine already written for devices and identities.
- **D5 Newcomer history.** Sponsor serves history from genesis vs from join. Recommend a genesis flag, default FROM JOIN: earlier writers never consented to a future member reading them. The founder flips it for a family molecule.
- **D6 Weave in molecules.** Weave with strand-pull vs weave nothing. Recommend the pull; weave-nothing is the fallback if the pull proves costly in the field.

## 8b. Settled in conversation (2026-09-14, Nick + Claude)

- The five build questions: era-pinned invites with sponsor refresh on mint; a JOIN also mints an era when the genesis policy is from-join (crypto boundary, not serve-policy); GroupEra wraps ride ONE ROW PER RECIPIENT DEVICE (a fat N×device row would ride the multi-packet PT path); per-member ACKs persist in the sender's pending ledger (the row keeps its any-ACK bool; an m_ack column can layer later); a never-friended member is addressed by folding their devices under the handle_proof and fetching the registry record by hp — retransmit runs on ACK absence, never liveness.
- Namespace: FOUNDER-SCOPED molecule proofs — `handle_proof("PHOTON_GROUP_v1" ‖ founder_proof ‖ name)` — name unique per founder, global squatting impossible by construction ("I can't have three purple turtle molecules but you could have one too"). Free-floating global molecule handles stay possible as keys-not-names but never the default.
- Attach tokens for join-by-link: bearer capability (knock route + sponsor), era-bound so it dies at the next mint; carried as ROUNDCODES (chameleon/verichrome — never QR) for optical desktop→phone, and NFC-tag programmable for standing invites. Registry learns only that a molecule proof exists and where a knock goes — roster and history never touch infrastructure.
- v1 ships registry-free exactly as §4 stands; names and tokens are the later attach layer.

## 9. Build order

1. Substrate: `MoleculeId`, roster records and merge, `author` column, chains blob off a molecule root, `MoleculePeer` trust. Unit: lane derivation from a molecule root, roster merge, writer discipline in a molecule, strand-pull DAG on a simulated three-member stream.
2. Genesis + invite over the pairwise braid + member record + three-fleet text with strand-pull + fleet replication. Field: three fleets, one member offline thru a session.
3. Eras: KEM bundles in member records, shrink mint on leave, cadence ratchet, the minter-id race.
4. UI: molecule row, author colours, delivered fraction, molecule page, D5 policy surface.
5. Later: molecule waves; add-consent gate; the relay fan-out if molecules outgrow per-device sends (fleet invariants say any size, so nothing here assumes small).

## 10. Lifecycle — the phase diagram (2026-09-15, Nick + Claude)

Decisions folded in here, superseding §2/§4/§6/§8 where they differ: **a molecule has no name in any registry** — its identity is the random id and lineage, and the title is one more roster record (`title`, signed by whoever changed it, newest wins, the genesis suggestion is the first); nothing to squat, nothing to sell, nothing freed on the last leave. **Membership is a friendship with more people**: a member brings you in over the friendship braid you two already hold, and you say yes by tapping Join on that offer — never a silent adoption, never join-on-first-send. Any member adds; nobody removes anyone but themselves; the unwelcome member is answered by a new molecule. Join-by-link (§8b) carries a random era-bound token, not a name. Open communities with claimable names are a different product, undecided.

### 10.1 The molecule as you hold it

| Phase | What it means | Edges out |
|---|---|---|
| **None** | You don't hold it. | A friend's offer row lands in your conversation with them → Offered. |
| **Offered** | A Join row in the friendship conversation: sponsor, title, member count, history policy, the era-pinned secrets in typed fields. No timer; the row stays as testimony. | Join → Joining. Sponsor refreshes on a mint → Offered (row updates in place). Sponsor leaves the molecule, or the friendship ends → Expired. |
| **Expired** | The person who vouched is gone. Row relabels; nothing else changes. | A fresh offer (from anyone standing) → Offered. |
| **Joining** | You hold chains + roster; your member record went out as your first frame on your lane. Under from-join the molecule also has to mint you an era. | Any standing member ACKs your record → Standing. No timeout. |
| **Standing** | The normal state. **Alone** is the same state with nobody else standing (the header says so). | Leave → Leaving. Era moved while you slept → Catching up. Everyone else leaves → Standing (alone). |
| **Catching up** | A wrap for the current era is in re-serve; current-era frames drop pre-decrypt until it opens. | Wrap opens → Standing. |
| **Leaving** | Your leave record is posted; compose disabled; history readable. | A survivor countersigns (the mint) → Left. Alone: at once. |
| **Left** | Read-only history, greyed row, winnowable. Terminal for this lineage. | A fresh offer → Offered (a re-join is a new member record; newest wins). |

### 10.2 Another member as you see them

Invited (sponsor-local only: "waiting on Emma") → Joined, name pending (record merged, fold not yet succeeded: gradient avatar, "Pending…") → Standing (named) → Departed (leave record merged; greyed in the member list; their rows stay attributed). Records never delete.

### 10.3 A row in a molecule

Yours: Composed → Sending (encrypted ONCE on your lane, the same ciphertext fanned to every standing member's devices) → Delivering k of n (the ring fills by the fraction of standing members that ACKed; n shrinks when a member departs) → Delivered (all standing ACKed). Replicated (a sibling holds it) runs beside, as today.
Theirs: Arrived → Parked (a woven strand is missing: pull it from the sender, or from any standing member if the sender is gone) → Applied → Notified.

### 10.4 Eras, and the only two places you see them

| Trigger | Who mints | What you see |
|---|---|---|
| A leave (countersign) | The first survivor to observe it; a race falls to the lowest party id | The leaver's row greys, then "left". |
| A join under from-join | Any standing member | Nothing; the joiner's Join completes. |
| The 256-row cadence | Any standing member | Nothing. |
| A device that slept thru a mint | — | "Catching up" until its wrap opens. |
| A sponsor holding a stale offer | — | The offer refreshes in place, or expires with the sponsor. |

### 10.5 Screens

- **Ready list.** Molecule rows beside contact rows, one list, sorted by recency of activity ("frecency"). Ring by phase: dashed while Joining, solid when Standing, grey when Left; unread ring as today. Avatar = composite of standing members' gradients.
- **Friendship conversation.** The offer row: title, "Emma brought you into taco · 4 people", Join. Relabels to "joined" / "offer expired". The sponsor's side shows "you brought Emma into taco · waiting / joined".
- **Contacts screen.** A filter strip (All · Friends · Atoms · Molecules) and **New atom**: the search box becomes the title box and the plus founds the atom, you alone in it. Founding never involves anyone else.
- **Contact panel → Manage.** "Bind into a molecule": pick an atom or molecule you stand in; the friend is offered a bond over your conversation with them.
- **Atom panel → About.** "Create a molecule!" — the Bind page, with the line beneath saying that the moment someone binds, this atom is a molecule.
- **Molecule conversation.** Header: title + count ("taco · 4"). Rows carry author name and colour (`molecule_id ‖ author`, so everyone sees the same colour for the same person). Your rows' delivered ring fills by fraction. Compose as today.
- **Molecule panel** (the orb inside a molecule conversation, same rail as the contact panel). About: title (editable by anyone, lands as a record), members by phase, history policy, era index for diagnostics. Add: your contacts not yet standing, one tap offers. Manage: mute (ships with the first row), detach title (later), Leave with a confirm.

## 11. Built (2026-09-15) — decisions D10–D14, the revocation model, and what is still open

Decisions settled at planning, superseding earlier sections where they differ:
- **D10 Secrets only ever move as wraps.** The OFFER carries a roster snapshot and nothing secret; the invitee's JOIN sends its member record + a per-device KEM bundle back over the friendship; the sponsor vouches, posts the records into the molecule, and answers with a WRAP — the era WHOLE (root ‖ history key) sealed to that bundle (era N under from-genesis; a freshly minted N+1 under from-join, wrapped to every standing device inside the molecule as well). A mint's wraps to existing members carry `fresh` (they derive N+1 from the era they hold). A wrap names its bundle (`bundle_id`) and is matched BEFORE any decapsulation — the HQC decapsulator aborts on a foreign key.
- **D11 Per-device `BundleRecord`** signed by the owning device, newest wins per (party, device); a device publishes a fresh bundle after every cutover. The private half is `EraDecapKeys` in that device's chains blob (one superseded bundle kept).
- **D12 `MoleculeLocal { muted, phase, offered }`** beside the roster, never inside it.
- **D13** Carriers: phase 2 = words + NFC; phase 3 = roundcode.
- **D14** Colour = `party_colour(spaghettify(molecule_id ‖ author))`, the same on every member's screen.

**Revocation = vouch and withdraw (the A5 branch).** `standing(X) = joined(X) ∧ ¬left(X) ∧ ∃ a live counting vouch for X`: a `VouchRecord { voucher, subject, withdrawn }` counts iff its voucher had joined and not left when it signed; every record is kept (a departed voucher's late withdrawal erases nothing); the founder stands on its genesis self-vouch and no other self-vouch counts. Anyone standing may vouch; you withdraw only your own. Losing standing does one thing — `standing = false` — from which the token drops the party's devices, the next mint omits them, and every pending stops owing them. The Withdraw pill is phase 1.5; every member verifying a mint's wrap set against its own roster (withholding a short one) is the exclusion tripwire that ships with it.

**Lifecycle as built.** Ready rows for molecules (the members' gradient pie, the molecule colour, the best presence tier over the members' rows) · the molecule conversation thru a transient contact view (title · n / just you / joining… / catching up… / you left; each incoming row in its author's colour with the author's name grant above it) · founding from a friend's Manage page ("Bring into a molecule": New molecule with title + policy, or an existing molecule — founding offers in the same act) · the offer card in the friendship conversation with its one Join pill (sponsor side: waiting / joined) · one ciphertext fanned to every standing member's device, a per-member ACK ledger (targets / acked_by, the implied-ACK rule per party, `delivered` at full coverage, n shrinks on a departure), retransmit only to the parties still owed · token-scoped admission at the RX door and the MoleculePeer fold for never-friended members · the roster riding the chains blob so fleet replication carries membership with the keys (a sibling boots a molecule it never held) and molecule rows pushing under the molecule token · the molecule panel (About: rename, policy, era, members by phase; Add; Manage: mute, two-tap Leave) · Leave = the signed leave record as the last frame on our lane, read-only at once, the root zeroized on its first ACK (alone: at once) · eras: mint on a lost standing and on the 256-row cadence (the lowest standing party id mints), the minter cuts over on the first wrap ACK, a receiver on the first frame tagged with the pending era, Catching up when a current-era frame will not open, and after any cutover the held rows re-queue and a fresh bundle publishes · Settings → Conversations: the explainer, the default newcomer-history policy for molecules you found, every molecule with a Mute pill.

**Still open.**
- Step 7: the live strand pull. Molecules send anchor-only until the `StrandLedger` is wired to the gap buffer with `strand_pull`/`strand_serve` on the history-page codec.
- Step 5's last piece: a frame from a never-friended member passes the door (token-scoped admission) but the receive commit still resolves the sender thru the contact fold and drops it. The commit path is contact-indexed thruout; the fix is a peer-view resolution there.
- A molecule control row (offer / join / wrap) is never re-served bare; a lost offer is re-sent by the sponsor on the next roster edge (the `offered` list) — that refresh is not yet wired.
- The delivered ring on a molecule row fills at full coverage only; a fractional fill from the ledger is a render change.
- Phase 1.5 (Withdraw), phase 2 (words, NFC, doors), phase 3 (roundcode, attachments, waves).
