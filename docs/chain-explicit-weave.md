# Chain ratchet: explicit-hash weave (design)

Status: **design only, not implemented.** Written 2026-06-28 from live both-sides logs (zeno ↔ phone p). Fixes the message-chain desync that survives the `34fc92d` weave-snapshot fix.

## The two coupled bugs (from live evidence)

The chain advance (`derive_fresh_link`, `src/crypto/chain.rs`) mixes: `DOMAIN + eagle_time + our_plaintext + chain_portion + their_plaintext`. The per-message salt (`derive_salt`) mixes: `DOMAIN + prev_plaintext + last 12 chain links`. For a message to decrypt, BOTH sides must agree on (a) `their_plaintext` (the woven peer message) AND (b) the chain advance position (which sets `chain_portion` + the salt links + current_key).

Live logs after the `34fc92d` weave-snapshot fix showed it advanced the failure from "msg 2 garbage" to "msg 3 garbage" — real progress (msg 2 now decrypts, weave was correct) — but two residual bugs remain:

1. **Weave selection is still implicit/"latest".** The sender picks which peer-plaintext to weave by an implicit "most recent" rule; under messages crossing in flight the two sides disagree on what "latest" is.
2. **Advance is ACK-timing-gated → stale-key reuse.** zeno's log showed the SAME key+salt (`ef1f5e04`/`561e2363`) reused for TWO consecutive received messages — the chain did not advance between them. Advancement is gated on ACK ordering (`CHAT: Chain advanced (ACK verified)`); when multiple peer messages arrive before the chain advances, message N+1 is decrypted with message N's stale key → garbage.

## The fix (user's vision): explicit-hash weave, deterministic per-message

Stop deriving anything from "latest" or from ACK timing. Every message carries an **explicit reference** to the prior message whose secret it weaves, and both sides derive purely from that named reference + the message's own position. Ordering-immune by construction.

### Reference type: HASH (msg_hp) — already on the wire
`incorporated_hp` (32-byte msg_hp) is ALREADY in every message and already resolved by the receiver via `get_pending_plaintext_by_hp`. So the wire field exists; the change is in SELECTION and DERIVATION, not the protocol header.

### Sender side
- When sending message N, **explicitly select** which prior received message to weave: a random pick K from the last up-to-256 received messages (entropy diversity; harder to predict the mixed secret even for guessable plaintext). K = that message's `msg_hp`.
- Put K in the message as `incorporated_hp` (the field already does this; today it's "latest", change to the random-from-256 pick).
- Derive the advance/secret from the plaintext of message K (looked up by hash), NOT from "latest".
- Snapshot K's plaintext onto the PendingMessage (the `34fc92d` mechanism already does this — keep it; it's what makes the sender's later ACK-advance use the same bytes).

### Receiver side
- Read `incorporated_hp` (= K) from the message. Resolve K's plaintext from ITS store of prior messages (by hash). This is what it already does at `photon_app.rs:5086-5093` — keep.
- **Decouple advance from ACK timing.** Advance the chain deterministically per received message using (the message's own position/seq) + (the explicitly-named weave K), so two messages arriving back-to-back each advance correctly instead of reusing a stale key. The message must carry enough to position itself in the chain (its sequence/prev_msg_hp — `prev_msg_hp` is already on the wire) so the receiver advances to the right link regardless of arrival order or ACK state.

### Why this kills BOTH bugs
- Bug 1 (weave): the woven message is NAMED in the header (K), not inferred — both sides resolve identical `their_plaintext`. Random-from-256 is SAFE precisely because K is explicit.
- Bug 2 (advance timing): advance keys off the message's own explicit position + named weave, not "advance when an ACK says so" — so N+1 never reuses N's key even under rapid/out-of-order delivery.

### Lookups need a hash-indexed window of prior messages
Both sides must resolve a `msg_hp` from up to 256 prior messages. `get_pending_plaintext_by_hp` today searches only OUR unacked pending (the sender's outbound). For the receiver to resolve a weave K (one of ITS prior messages) and for random-from-256, we need a hash-indexed ring of the last 256 messages per chain (plaintext + msg_hp + seq), on BOTH the sent and received sides. This is the main new storage. Persisted so it survives restart (the salt/weave already need last_plaintext across restart — extend that to a 256-window).

## Open questions for implementation
- Exact "position" the receiver uses to advance deterministically: seq counter vs prev_msg_hp chain-walk. prev_msg_hp is already on the wire and already used for hash-chain continuity — likely the anchor.
- Gap handling: if K (the woven message) or a skipped message isn't yet present (out-of-order), buffer until resolvable (there's already a `gap_buffer` in FriendshipChains for out-of-order prev_msg_hp).
- Window size 256 and eviction (ring). Restart persistence of the window.

## Sequencing
Design only for now. Implementation is a real ratchet change — do it with the live zeno↔p repro to validate every message N decrypts, including rapid-fire and crossed-in-flight, not just the happy 1-at-a-time path.
