# Lanes — one ratchet per device, zero forks by construction

**Status:** IMPLEMENTED (2026-08-03) — substrate, wire label, receive-anywhere, and the lane-wise replication merge are live; frames carry labels and the whole-blob adopt is gone. The checkpoint transport is the existing chain-replication blob (a blob = the set of lane checkpoints it holds, merged position-wise). Era supersede is in: a re-keyed `lane_root` replaces a sibling's old-era blob wholesale (newer `mutated_osc` wins) instead of lane-merging, which would have stranded it on dead chains. Remaining from this spec: per-lane label rotation at epoch bumps, and the fork-on-single-writer-lane loud-error (today the pre-lane garbage-decrypt streak repair stands in). Companion to `braid.md` §14 (which sketches the fleet plane broadly); THIS document is the concrete design the code will follow. Supersedes §14 where they disagree.

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

Chains schema **v8**: lanes + `lane_root`. v≤7 blobs read as **absent** → the contact re-CLUTCHes through the existing re-key flow. Conversation rows are untouched (identity-keyed). This rides the same flag-day as the binary-numeral domain flip (shipped) — one re-clutch per pair covers both. Per standing doctrine: re-clutch always; the only new secret at rest is `lane_root`, which lives and dies with the chains blob it seeds (same custody as the 16KB of chain links beside it — no new exposure class).

## Verification

- Unit: lane derivation is label-deterministic (two sides, same `lane_root` + label → identical lane); writer discipline (advancing a non-minted lane is a panic in debug, an error in release); checkpoint adopt-iff-greater; replay convergence (two simulated devices fed the same frame stream converge to identical lane state); flag-day (v7 blob reads absent, re-clutch fires).
- Live (two-device fleet): message from a friend renders on BOTH devices and whichever is awake ACKs; kill the phone mid-conversation → desktop continues alone; wake the phone → checkpoint fast-forward, no fork detector, no re-key. Pull logs and read the `LANE:` lines.

## Out of scope here

Groups (Phase C — same lanes off a group root), the reservoir/epoch FS machinery of §14.10 (lanes are compatible with it; it layers on later), UI (no UI work in this phase — rendering still shows one conversation, lanes are transport plumbing).
