# Groups — a shared root delivered pairwise, lanes off it, sovereign membership

**Status:** PROPOSAL 2026-09-14, not built. Written against the shipped substrate: per-device lanes (docs/lanes.md), the era ratchet (`crypto/era.rs`), the fleet-key fan-out shape (docs/fleet-key.md), fleet-holds-history (docs/durability.md), and the participant-set `Conversation` (`types/conversation.rs`). lanes.md promised "Phase C — same lanes off a group root"; this is that phase, spelled out. The word "groups" in docs/global-vault.md means device groups for settings linkage and is unrelated.

## 0. The one-paragraph model

A group is a conversation with a mutable member set and its own root secret.
There is no group ceremony: the founder mints the root, and every later member receives it from a sponsor over the pairwise braid the two of them already hold.
Two members who never CLUTCHed each other share only the group root, and that is enough: every device of every member derives its lane from the root and a label, exactly as a friendship does today (lanes.md), so receive-anywhere, writer discipline, checkpoints and fleet replication carry over verbatim.
Membership is sovereign records: a member's standing in the group is its own signed record, others verify or withhold; joining is posting that record, leaving is the leaver's signed request plus one survivor's countersign, and the countersign mints the next era so the leaver holds no future key.
Expulsion is not a verb; the group verb is "not in the next key", and the remedy for an unwanted member is a fork (a new group without them).
No group state ever touches infrastructure: the group holds its own history, and the fleet plane replicates each member's copy across their devices as it does for any conversation.

## 1. Why this shape and not the other two

**Not the N-party CLUTCH.** clutch.md §3.4/§4.1 already sketch a slot ceremony for N handle hashes, and `PartySlot` is N-party in the type. It should stay unused for groups: every member's fleet must complete the ceremony before anyone can speak (a dormant fleet blocks the whole group), each member posts a 550 KB offer to N−1 peers, and every membership change re-runs all of it. It also demands that every member know every other member's handle, which is the very thing groups exist to avoid.

**Not pairwise fan-out** (encrypt N−1 times, one per friendship braid). Simplest crypto, but it requires a full mesh of friendships: you could not add your cousin to the family group unless your sibling had CLUTCHed with them too. The acid test for any group design here is "I add my contact; nobody else needs to know them", and pairwise fan-out fails it. It also advances N−1 lanes per send and makes delivery N separate stories.

**The shared root** is the sender-keys shape (Signal groups, MLS), and photon already speaks it: the fleet key is a shared secret fanned out per member and re-minted on shrink (fleet-key.md); lanes are a shared root plus a public label; eras are a two-phase root ratchet with retired-era stragglers. Groups reuse all three with a different root.

## 2. Identity

- **Group id.** `group_id = blake3("PHOTON_GROUP_v1" ‖ genesis_nonce)`, 32 random bytes minted by the founder. It is deliberately NOT participant-derived: `FriendshipId::derive` keys a conversation on its participant set, which is immutable by construction, and a group's set is not. `Conversation` gains a stable-id form whose participant vec is mutable behind that id; everything downstream (rārangi table, chains blob, conversation state) keys on the id bytes as today.
- **Party id.** A member's party id is the same opaque 32-byte pinned identity pubkey friends use in `Contact.handle_hash`. A member you never friended: you learn their party id, their `handle_proof`, and (by folding the public membership chain under that proof, as `Contact` already does) their current device set. You never learn their handle; handles are keys and stay grants-only (docs/identity-profile.md).
- **Names.** A member grants the group its published name and avatar pin as part of its member record (the pong's always-granted name slot, scoped to the group). Petnames stay local. Names carry zero trust, the pinned key does.
- **Group token.** `group_token = blake3_derive("photon.group.token.v1", group_id)`, the routing tag on every group frame (the `conversation_token` role). Only members hold `group_id`, so only members can mint or match it. It is stable across eras; the era tag rides beside it as it does for friendships.
- **Transport trust, scoped.** Today every inbound frame is contact-gated (`knows_device`, `answerable_pubkeys`). A group adds a second trust source: a `GroupPeer` (party id → folded devices) beside `Contact`, consulted ONLY for frames carrying that group's token. A non-friend member's pings still drop; presence outside the group is not granted by membership.

## 3. Keys

**Root.** Each era has `group_root` and `group_history_key`, both 32 bytes; `era_lineage` is the one-way image of the era-0 root, `era_index` monotonic, exactly the friendship era model.

**Lanes.** `lane = expand(group_root ‖ "PHOTON_LANE_v" ‖ [1] ‖ label)`, lanes.md verbatim with the group root as the seed. Each of my devices mints its own label at its first send in the group; any receiver derives the lane from the label alone. One writer per lane, so no fork; checkpoints replicate across my fleet on the same chain-replication plane, keyed by `group_id`.

**Frames.** The chat frame's `msg` section gains `from` (the sender's party id; in a friendship it is implicit, in a group it is attribution) beside the era tag. The outer Ed25519 signature is verified against the sender's folded devices from the roster. One ciphertext per row, sent to every member's active device over the same LAN → WAN → relay ladder friendships use; each member's fleet replicates inward from whichever device received it.

**The weave.** A sender weaves up to two rows it has RECEIVED, from any member (braid §6). Weave references become `(author party id, eagle_time)`, because eagle times are unique per device, not per group. A receiver that holds the frame but not a woven strand cannot derive the link, so it parks the frame in the gap buffer (already keyed on a missing predecessor; this adds "missing strand") and PULLS the strand from the frame's sender, who necessarily holds it: `strand_pull`/`strand_serve`, one row sealed under the group history key with the history-page codec. Weave references point strictly backwards in causality, so the pull graph is a DAG and never deadlocks; it is edge-driven (on the parked frame) and never timered. This keeps braid §1.4's confluence promise in groups: any delivery order converges.

**ACKs and the sender's advance.** Receivers advance on receipt as today. The sender's lane advances on the first ACK from any member (the advance is a pure function of the row, so which member ACKed is irrelevant); the pending entry keeps retransmitting to the members that have not ACKed, and re-serve covers the rest. `delivered` on a row becomes the set of party ids that ACKed it.

**Eras.** Same three states (current, retired read-only for stragglers, pending two-phase). Next-era secrets come from the existing `derive_era_keys(group_id, idx, old_root, old_hk, fresh, transcript)`. What changes is how `fresh` reaches N fleets that share no pairwise channel: every device publishes, in its member record, a group-scoped hybrid KEM bundle (ML-KEM-1024 + X25519 + HQC-256, `era_keygen` as it exists) whose decapsulation keys live in the group chains blob, the same custody class as the 16 KB of lane links beside them, and which the device replaces on its first frame of every new era. The minter encapsulates `fresh` to every surviving device's bundle and posts a `GroupEra` control row on the OLD era carrying the wraps (the PFO1 blob shape, KEM in place of ECDH). A device that was offline reads the row from history re-serve; the retired era stays readable for exactly that. Triggers: SHRINK (mandatory, see §4), the 256-row cadence (any member may propose), never a heavy weave (there is no CLUTCH to weave). Two proposers for the same index resolve by lowest minter party id; the loser drops its pending, no owner election (clutch.md §10.1 ruling). Stated honestly: a leaked chains blob leaks that device's decapsulation key, which opens the next era's wrap to it, until its bundle rotates at that era.

## 4. Membership — the roster as sovereign records

The roster is a set of signed records, merged by union with subject-signed newest-wins per party. It rides inside the group as hidden control rows (`GROUP_PREFIX`, the `ERA_PREFIX` pattern), so it is history, re-servable, and fleet-replicated with no new plane.

- **Genesis** — founder-signed: `group_id`, lineage, founder party id, genesis eagle time, and the newcomer-history policy (§8 D5).
- **Member** — signed by one of the member's own devices: party id, `handle_proof`, group name grant, avatar pin, per-device KEM bundles, `joined_osc`, sponsor party id. Everyone folds the member's devices from the public membership chain under the proof; no device list is trusted from the record itself.
- **Invite** — the sponsor's pending record, EXPIRING, sent PAIRWISE over the sponsor↔invitee friendship braid as a control row: `group_id`, lineage, the current era's root and history key, and a roster snapshot so the invitee sees who is in it before consenting. The secrets ride TYPED package fields (`gei`/`groot`/`ghk`/`glin`), consumed at ingress and never written to the row: the row text persists, replicates to siblings and re-serves as history, so a secret in it would outlive its era and defeat the retired-era zeroize (fixed 2026-09-15, before any group was ever minted). The text is the bare kind marker. The invitee's consent IS its Member record, posted into the group as its first frame. Any member may sponsor any contact of theirs; a rejected or expired invite leaves no record.
- **Leave** — the leaver's signed request. The first survivor to observe it countersigns and mints the next era, wrapping to every device except the leaver's; a second countersigner loses on minter id and adopts. Departure therefore always costs one mint, exactly as a fleet shrink does.
- **No expulsion.** Consistent with docs/pairing-v2.md: verbs of self (request, consent, resign) and verbs of group (verify, include in the next key, exclude from the next key). "Exclude from the next key" without the subject's request is a fork; v1 does not offer it as a button. The remedy is a local mute and, if needed, a new group.
- **Ostracism, never erasure.** A left member's records stay as testimony; its rows stay in history.

## 5. The fleet plane, unchanged

My devices see a group as one more conversation: chains blob (lanes, eras, KEM decaps) at `vault_key("chains", group_id)`, rows in the rārangi table `group_id`, the roster at `vault_key("group", group_id)`. Checkpoints, sibling row pushes, history pages, anti-entropy digests and the delivery ladder apply as they stand. Rows gain one additive column, `author: PartyId` (absent on old rows = derived from `is_outgoing`). No group slot on FGTW, no worker change: durability.md's option B by analogy, group-holds-history.

## 6. UI

- Contacts list: a group row, avatar = a composite of member gradients (no orb of its own), unread ring as today.
- Conversation: author name and `party_colour` of the author's party id on each row; the delivered ring fills by the fraction of members that ACKed. Header lists members.
- New group: pick from your contacts; a group page with members, Add (your contacts only), Leave.
- Honest surfaces: "history before you joined isn't shared" when D5 is off; "N members haven't received this yet" is the delivered fraction, nothing timered.
- Calls in groups are out of scope for v1 (calls.md is pairwise).

## 7. Concrete additions

- `types/group.rs`: `GroupId`, roster records + merge, `GROUP_PREFIX` control-row grammar (genesis, member, invite, leave, era).
- `Conversation`: stable-id form with a mutable participant set; `ChatMessage.author`.
- Frame: `from` party id; weave refs `(author, eagle_time)`; `strand_pull`/`strand_serve` on the history-page codec.
- Chains blob schema v9 (additive): group flag, per-device KEM decapsulation keys, lanes off the group root.
- `GroupPeer` trust source consulted for group-token frames only.
- Storage: `vault_key("group", group_id)`; rārangi table = `group_id`.

## 8. Decisions for Nick, with a recommendation each

- **D1 Shape.** Shared root delivered pairwise (this doc) vs N-party CLUTCH. Recommend the shared root, §1.
- **D2 Id.** Stable random `group_id` vs participant-derived. Recommend stable; membership moves, the conversation must not.
- **D3 Adds.** Any member sponsors any contact of theirs, no vote. Recommend yes for v1; a consent gate on adds can layer later as a genesis flag.
- **D4 Expulsion.** None; fork is the remedy. Recommend yes, it is the doctrine already written for devices and identities.
- **D5 Newcomer history.** Sponsor serves history from genesis vs from join. Recommend a genesis flag, default FROM JOIN: earlier writers never consented to a future member reading them. The founder flips it for a family group.
- **D6 Weave in groups.** Weave with strand-pull vs weave nothing. Recommend the pull; weave-nothing is the fallback if the pull proves costly in the field.

## 8b. Settled in conversation (2026-09-14, Nick + Claude)

- The five build questions: era-pinned invites with sponsor refresh on mint; a JOIN also mints an era when the genesis policy is from-join (crypto boundary, not serve-policy); GroupEra wraps ride ONE ROW PER RECIPIENT DEVICE (a fat N×device row would ride the multi-packet PT path); per-member ACKs persist in the sender's pending ledger (the row keeps its any-ACK bool; an m_ack column can layer later); a never-friended member is addressed by folding their devices under the handle_proof and fetching the registry record by hp — retransmit runs on ACK absence, never liveness.
- Namespace: FOUNDER-SCOPED group proofs — `handle_proof("PHOTON_GROUP_v1" ‖ founder_proof ‖ name)` — name unique per founder, global squatting impossible by construction ("I can't have three purple turtle groups but you could have one too"). Free-floating global group handles stay possible as keys-not-names but never the default.
- Attach tokens for join-by-link: bearer capability (knock route + sponsor), era-bound so it dies at the next mint; carried as ROUNDCODES (chameleon/verichrome — never QR) for optical desktop→phone, and NFC-tag programmable for standing invites. Registry learns only that a group proof exists and where a knock goes — roster and history never touch infrastructure.
- v1 ships registry-free exactly as §4 stands; names and tokens are the later attach layer.

## 9. Build order

1. Substrate: `GroupId`, roster records and merge, `author` column, chains blob off a group root, `GroupPeer` trust. Unit: lane derivation from a group root, roster merge, writer discipline in a group, strand-pull DAG on a simulated three-member stream.
2. Genesis + invite over the pairwise braid + member record + three-fleet text with strand-pull + fleet replication. Field: three fleets, one member offline thru a session.
3. Eras: KEM bundles in member records, shrink mint on leave, cadence ratchet, the minter-id race.
4. UI: group row, author colours, delivered fraction, group page, D5 policy surface.
5. Later: group calls; add-consent gate; the relay fan-out if groups outgrow per-device sends (fleet invariants say any size, so nothing here assumes small).
