# Lanes — one ratchet per device, zero forks by construction

**Status:** IMPLEMENTED (2026-08-03) — substrate, wire label, receive-anywhere, and the lane-wise replication merge are live; frames carry labels and the whole-blob adopt is gone. The checkpoint transport is the existing chain-replication blob (a blob = the set of lane checkpoints it holds, merged position-wise). Era supersede is in: a re-keyed `lane_root` replaces a sibling's old-era blob wholesale (newer *genesis* wins — the dead era's `mutated_osc` keeps ticking) instead of lane-merging, which would have stranded it on dead chains. RESIDUE CLOSED (2026-08-18): label rotation at the re-key epoch is complete on all three legs — owner re-key (fresh blob), sibling era-adopt (`our_label` dies in the supersede, re-mint at next dispatch), and wedge rotation (`rotate_our_lane`, soak-hardened) — with a tripwire test (`old label must NOT survive the supersede`). Writer discipline is now ENFORCED at `prepare_send_commit` (debug panic / release refusal + `LANE VIOLATION` log), and the receive-side fork evidence logs the loud `LANE FORK (single-writer!)` line naming lane + position + era; the convergence-first streak ladder remains as the healer for era stragglers. Companion to `braid.md` §14 (which sketches the fleet plane broadly); THIS document is the concrete design the code will follow. Supersedes §14 where they disagree.

## The problem being solved

A friendship today holds one chain per **identity**. A fleet is many devices behind one identity, so "our" chain has many would-be writers — and a ratchet with two writers forks. Everything downstream of that fact is patchwork: the §4.2 ceremony-owner lease, whole-blob chain replication with newest-`mutated_osc`-wins (a fork window, not a fix), the compose-anywhere fleet-forward dance, and the fork detector that heals what should never have happened.

The fix is structural: **a lane is one device's ratchet inside a conversation. Each device advances only the lane it minted. No lane ever has two writers, so no lane can fork.**

## The model

A conversation's chains blob holds:

- **`lane_root`** — a 32-byte secret derived once at ceremony birth from the pairwise avalanche (the `history_key` precedent: identical on both sides exactly then, retained in the blob, zeroized on supersede). It is the seed every lane grows from.
- **Lanes**, each `(label, chain, position, last_plaintext, hash-chain state)` — created on demand, one per `label` ever seen.

### Lane derivation — device identity never enters

```
lane_label  = 32 random bytes, minted by the sending device at its first send ("at-weave")
lane_chain  = expand( lane_root ‖ "PHOTON_LANE_v" ‖ [1u8] ‖ lane_label )   → 8KB active portion, same expansion shape as chain derivation today
```

The label rides **every frame** on that lane, in the clear. Anyone holding `lane_root` — both fleets' devices, nobody else — derives the lane **from the label alone**. This one property pays for everything:

- **Receive anywhere.** Any of our devices can decrypt and ACK any inbound frame: label on the wire → lane from `lane_root`. No fold lookup, no trial decryption, no "which device is this" question at decrypt time.
- **Pseudonymity (plan item 8), actually achieved.** The wire never carries a pubkey-derived value; a lane label is unlinkable to a physical device by construction, because device identity is not an input anywhere in lane derivation. Friends learn the lane *count* (the fold already tells them the device count); mapping label→device would take live traffic correlation, which transport addresses already permit — the *record* stays clean. Local logs may name devices; the wire never does.
- **New device, no ceremony dependency (plan item 7).** A device that joins either fleet receives the chains blob (ours: fleet replication; theirs: nothing — their fleet replicates their own), mints a label, sends. The receiver has never heard of the device and does not need to: `lane_root` + label is sufficient. The fold still gates *trust* (whose frames are honoured at the transport layer) — it just no longer gates *lane existence*.
- **No collision case.** Labels are 256-bit random; two devices cannot mint the same one. There is no claim protocol, no slot index, no tie-break.

A device mints a **fresh label** after a re-key epoch (the old lane retires at its final checkpoint), so labels also never outlive the key material under them.

### Writer discipline

One rule, enforced in code, testable: **a device advances only the lane whose label it minted.** Every other lane is receive-only on this device. Sibling state for *our other devices'* lanes arrives exclusively via checkpoints — never by locally advancing their lanes.

## Checkpoints — replication without the fork window

Today the whole chains blob replicates across the fleet, newest-`mutated_osc`-wins: two devices that both advanced "the" chain race to overwrite each other. With lanes, replication shrinks to **per-lane checkpoints**:

```
checkpoint = ( lane_label, position, chain 16KB, last_plaintext, last_received_hash )
sealed under the fleet key, pushed on advancement EDGES (post-ACK, post-receive-advance) — never timers
```

**Adoption rule: strictly greater `position` on that lane, else reject.** A checkpoint is a fast-forward of a deterministic replay, so adoption is always safe; equal-or-lesser is always stale. This replaces whole-blob newest-wins and deletes its fork window. The transport is the existing chain-replication plane (`drive_chain_replication` / the sibling adopt path) with the payload swapped.

Ordering dependency (plan item 5): a checkpoint can reference strands (woven message content) the adopting device hasn't merged yet — adopt checkpoints **after** the row merge for the same span, log a `LANE:` wait state otherwise.

## Send and receive

- **Send** is today's shape on *our own* lane: `prepare_send`, pending, ACK advances. The chainless-device fleet-forward becomes a fallback used only while this device's lane is still weaving, then dead code to delete.
- **Receive** resolves the sender lane by label (creating it if new), verifies the lane's hash chain, decrypts, ACKs — on whichever of our devices got the frame first. Duplicate ACKs from two of our devices racing the same frame are harmless today for the sender (dedup by eagle_time + re-ACK is free) and stay so.
- **ACK processing** for our outgoing messages: any of our devices may *receive* the ACK, but only the minting device advances the lane — a received ACK for a sibling's lane is forwarded to it via the fleet (or simply carried by the next checkpoint from the device that also saw the ACK; both are edges).
- The **weave probe** and **fork detector** keep their roles, per-lane. A fork on a single-writer lane is by definition evidence of key compromise or a replay bug — it becomes a loud error, not a heal path.

## What stays, what dies

**Unchanged:** `friendship_id` (identity-derived — conversation rows, history, UI stay keyed as today), `conversation_token` (identity-derived), the CLUTCH ceremony itself (still pairwise, still one per friendship — §4.2 ownership survives for the *ceremony* only), history recovery (key-agnostic pages), rārangi rows.

**Dies with lanes:**
- `FriendshipChains::other_participant` and the "not a 2-party chat" receive bail — the sender is the lane, not "the other one".
- The §4.2 lease *for messaging* (any device sends on its own lane).
- Whole-blob chain replication and its `mutated_osc` race.
- The compose-anywhere fleet-forward (fallback first, deleted after the weave-window proves out).

## Flag-day

Chains schema **v8**: lanes + `lane_root`. v≤7 blobs read as **absent** → the contact re-CLUTCHes thru the existing re-key flow. Conversation rows are untouched (identity-keyed). This rides the same flag-day as the binary-numeral domain flip (shipped) — one re-clutch per pair covers both. Per standing doctrine: re-clutch always; the only new secret at rest is `lane_root`, which lives and dies with the chains blob it seeds (same custody as the 16KB of chain links beside it — no new exposure class).

## Verification

- Unit: lane derivation is label-deterministic (two sides, same `lane_root` + label → identical lane); writer discipline (advancing a non-minted lane is a panic in debug, an error in release); checkpoint adopt-iff-greater; replay convergence (two simulated devices fed the same frame stream converge to identical lane state); flag-day (v7 blob reads absent, re-clutch fires).
- Live (two-device fleet): message from a friend renders on BOTH devices and whichever is awake ACKs; kill the phone mid-conversation → desktop continues alone; wake the phone → checkpoint fast-forward, no fork detector, no re-key. Pull logs and read the `LANE:` lines.

## Eras — a re-key never blanks the friendship (2026-09-08, stage 1 of the era ratchet)

An **era** is one generation of a friendship's keys: one `lane_root`, one `history_key`, every lane derived from them.
Eras carry an `era_index` (monotonic within a lineage) and an `era_lineage` (a one-way image of the era-0 root: a woven transition inherits it, a fresh CLUTCH mints a new one), and each lane is stamped with the tag of the root it derives from (never the index: a fresh ceremony over an existing friendship is index 0 again).
`clutch::era_tag` gives an era a 4-byte public fingerprint (the `s_id` pattern) for the wire and the logs.

Two neighbours of the current era may exist on a blob at once.
**Retired** is the previous era, kept read-only: its lanes still decrypt and ACK the peer's stragglers (frames they sent before their own completion), nothing is ever sent on them, and it is dropped — keys zeroized, lanes removed — on an observed edge: `RETIRED_ERA_GRACE_ROWS` current-era frames from the peer, or the next cutover. Never a timer.
**Pending** is the next era, derived but not yet written to: phase one of a two-phase cutover, used by the in-band ratchet (a later stage); `cut_over_to_pending` flips it to current with the `rotate_our_lane` shape (our lane and pendings reset, undelivered rows re-serve on a fresh lane under the new root).

A completed ceremony **supersedes** (`supersede_with`): the fresh era becomes current and the one we held becomes retired.
Nothing is destroyed at ceremony START any more — the offer that used to `friendship_id.take()` and delete the chains now only resets the ceremony round, so sends, express call signals and the compose bar ride the current era until completion.
This is what Emma's 2026-09-08 log paid for: `cannot send — no friendship chain`, `express frame opened by no friendship`, and a compose bar replaced by the ceremony ladder, all because a re-CLUTCH wiped the live chain the moment it decided to run.

Era order (`era_superseded_by`): within one lineage the index is the truth; across lineages the newer genesis wins; the equal-genesis legacy tie falls to root byte-order, as before.
A sibling whose current root is our pending root has cut over ahead of us — we follow it (`other_is_our_pending`), a "cut over now" rather than a supersede.
Sibling replication carries all of it (`replication_subset`), so a fleet converges on eras the same way it converges on lanes.

Storage is additive in schema v8 (`era_index`, `era_lineage`, per-lane `lane_era`, `retired_*`, `pending_*`); a blob without them loads as era 0 of the lineage its own root names, every lane in it.

**On the wire (stage 2, 2026-09-08).** The chat frame's `msg` section carries the sender's era tag (`era`, a name-keyed field a legacy parser never looks for): a known label routes by label, an unknown label materializes under the era the tag names, and a tag matching nothing we hold is dropped BEFORE decrypt and never counts as fork evidence. No tag = a pre-era peer = current era, garbage still to the fork detector as before. The pong's sealed tail gains an `era` row (index + tag per conversation), so a stale era is visible on the presence edge before any frame fails; the observation runs on a change edge per peer device through `era::repair_dispatch`, and a peer that advertises OUR era after delivering a current-era frame is the retire edge for the previous one. `chain_pull` became an `era_pull` when the asker holds chains (`era` = the index it holds): siblings serve iff they hold a NEWER era, and an all-miss answer is a repair-decision input, never the wipe-debris re-key. A contact latches `peer_era_capable` on its first tagged frame; the in-band ratchet may only be initiated toward such a peer.

**The light ratchet (stage 3, 2026-09-08; `crypto/era.rs`, `photon_app/era.rs`).** The next era's secrets are `KDF(old lane_root ‖ old history_key ‖ fresh ‖ transcript)`: the old root buys continuity and splice-resistance, the fresh secret — a hybrid of ML-KEM-1024, X25519 and HQC-256 — buys every bit of the new era's confidentiality. A departed device holding the whole old era lacks the initiator's ephemeral decapsulation keys (RAM only, zeroized on drop) and the responder's encapsulation randomness, so it cannot follow.
Three hidden control rows (`ERA_PREFIX`, `EraSignal`): the initiating identity (the lower party id, `participants[0]`) sends **Init** on the current era with its public keys in the package's typed `ekn`/`ekx`/`ekh` fields; the responder encapsulates, derives, installs the new era as PENDING with `resp_osc` = its **Resp** row's eagle time, and keeps chatting on the old era; the initiator decapsulates, derives and cuts over at once (the Resp proves the responder holds the era); the responder cuts over on the first of the Resp's ACK or the peer's first frame tagged with the pending era.
The identity that may not initiate sends a **Nudge** instead. A duplicate Init gets the cached Resp back (re-encapsulating would mint a second era for one nonce); a Resp that does not echo the nonce, era and prior tag in flight is dropped; a restart aborts an in-flight ratchet and the next edge proposes afresh. Sibling replication carries the pending era and the cutover (`replication_subset`, `merge_lanes_from`).
Edges only: the 256-row cadence (`LIGHT_RATCHET_CADENCE_ROWS`, a multiple of the peer's rows on one era), a nudge, or a `friendship_repair` verdict.

**The ceremony owner is computed (stage 3).** `era_owner` = the lowest device pubkey among fold members that are not locked and not probed-offline; an unprobed sibling still holds (the boot-race rule). Recomputed on the evidence edges — a sibling's presence verdict, a fold adopt, a locked-set change, a roster adopt — and written to every friendship's `ceremony_owner`; a round this device holds for a friendship that moved to another owner is discarded. Only the owner answers a friend's offer (the rest park and take the result by chain-sync) and only the owner proposes a ratchet; the claim-on-pickup, its roster LWW race and the departed-owner sweep are gone.

Not yet (later stages): the woven full CLUTCH on fleet shrink (`HeavyWeave`, logs); the consent-gated fresh channel (`ConsentFresh`, logs); the remaining ad-hoc triggers routed through `friendship_repair`; KEM decapsulation off the UI thread (inline today — a few milliseconds once per 256 rows).

## Out of scope here

Groups (Phase C — same lanes off a group root), the reservoir/epoch FS machinery of §14.10 (lanes are compatible with it; it layers on later), UI (no UI work in this phase — rendering still shows one conversation, lanes are transport plumbing).
