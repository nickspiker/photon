# The braid: explicit-hash weave (design)

Status: **IMPLEMENTED** (commit 9bf1193) — this file is the design rationale; the authoritative spec of what shipped is [CHAIN.md](../CHAIN.md) ("The Braid", v0.1). Written 2026-06-28 from live both-sides logs (zeno ↔ phone p) to fix the message-chain desync that survived the `34fc92d` weave-snapshot fix. The implementation diverged from this design in two ways worth noting: the weave reference is **eagle_time** (not msg_hp), and the "last 256" window is the **message-DB tail** (no separate ring).

## Name: the braid (not a ratchet)

This construction is the **braid** — not a ratchet. A ratchet only advances forward (monotonic, one-way); the braid reaches BACK into history and cross-weaves the peer's strand into our chain — bidirectional cross-entropy. The name is just "braid"; the window size is a parameter, not part of the name (don't bake a number in — it's tunable).

What makes it categorically different from a double ratchet is the **reach**: a double ratchet's weave depth is 1 (it mixes only the immediately-preceding step). The braid's source is a **WINDOW** of recent messages (currently the last 256) — each message braids in exactly ONE prior peer message, but K (which one) is chosen at random from anywhere in that window, so which prior secret gets mixed is unpredictable. Window, not count: depth-1 weave per message, source ranging over the window.

("Confluent" describes a PROPERTY, not part of the name: the explicit-hash references make any delivery order converge to the same braid state — Church-Rosser for the chain. Kept as rationale below, not in the term.)

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

## The weave derivation: hash = pointer, plaintext = ingredient

Settled mental model: **`incorporated_hp` (hash) is the on-wire POINTER naming which message is woven; the literal PLAINTEXT of that message is the INGREDIENT mixed into the secret.** The plaintext is NEVER sent — both sides already hold message K (peer authored it, we received+ACKed it). The hash carries no entropy (it's public + derivable); it only lets both sides agree WHICH plaintext to feed. So "take the literal message and append it" is exactly the mechanism — and it's already what `derive_fresh_link` does; the bug was only feeding a DIFFERENT plaintext on each side.

### Field order + framing (REQUIRED, both sides byte-identical)
Current `derive_fresh_link` (src/crypto/chain.rs:315) feeds `spaghettify(input)` where input = `DOMAIN + eagle_time + our_len+our_plaintext + chain_portion + their_len+their_plaintext` (ours first). REWRITE to lead with peer entropy:

```
spaghettify(
    DOMAIN_ADVANCE
    + eagle_time            (8 bytes, fixed LE)
    + their_len (4 LE) + their_plaintext   ← PEER message K's plaintext, FIRST
    + chain_portion         (fixed 256*32, the active links)
    + our_len  (4 LE) + our_plaintext
)
```

Rationale: the advance feeds the CUSTOM `spaghettify` mixer (not BLAKE3 directly). For BLAKE3 alone field order is security-neutral (avalanche is complete), but for a custom mixer leading with the high-entropy, hardest-to-predict input (the peer's woven plaintext) is the robust default — it avalanches the fixed/known portion (our plaintext, domain, chain links) rather than letting predictable bytes set a known prefix. Same principle as salt-before-password in a KDF.

NON-NEGOTIABLE: keep LENGTH PREFIXES (or fixed delimiters) on every variable-length field. Injective/canonical framing is what prevents a concatenation collision (else "AB"+"C" and "A"+"BC" hash identically). Order is a free choice; unambiguous framing is not.

This is a deliberate advance-derivation change: BOTH sides must change identically (any mismatch = total chain desync), and it makes all pre-change chains incompatible (dev: nuke+re-key; release: version bump).

### Sync rules (decided)
- **Weave K = a PEER message we've ACKed** (guaranteed both-held: they authored it, we received-and-acked it — so the SENDER knows the receiver holds it; survives reorder/restart). The eligible SET is the last 256 **ACKed peer messages** (recency eviction among ACKed ones — the ring tracks ACKed-ness, not just receipt). Sender restricts the pick to this set, so it can never reference something the receiver lacks → no buffering/gap dependency for the weave. Bidirectional entropy (peer's content into our chain), the existing `incorporated_hp` intent.

### Selecting K — random index, NOT modulo
`window = min(acked_peer_count, 256)`. Then:
- `window == 0` (no ACKed peer message yet — brand-new conversation): weave **nothing**, `their_plaintext = None`. The anchor/first-message case, same as today.
- `window >= 1`: `K = candidates[csprng.gen_range(0..window)]` — a random index BOUNDED BY `window`. The range naturally IS `window` (5 messages → pick 1 of 5; 32 → 1 of 32; 300 → 1 of the last 256). **Do NOT use `random % 256`**: it indexes past a small `n` (OOB), and `% n` reintroduces modulo bias. `gen_range(0..window)` is uniform + bounded + bias-free.

**Selection is NON-DETERMINISTIC (CSPRNG/TRNG).** This is SAFE precisely because K is named explicitly on the wire (`incorporated_hp`): the receiver never guesses K, it reads the hash and looks it up. So the randomness costs nothing to sync and buys defense-in-depth — an attacker with partial state can't predict which prior secret mixes into the next braid step. Randomness helps the sender; the explicit hash makes it free for the receiver.

## Open questions for implementation
- Exact "position" the receiver uses to advance deterministically: seq counter vs prev_msg_hp chain-walk. prev_msg_hp is already on the wire and already used for hash-chain continuity — likely the anchor.
- Gap handling: if K (the woven message) or a skipped message isn't yet present (out-of-order), buffer until resolvable (there's already a `gap_buffer` in FriendshipChains for out-of-order prev_msg_hp).
- Window size 256 and eviction (ring). Restart persistence of the window.

## Sequencing
Design only for now. Implementation is a real ratchet change — do it with the live zeno↔p repro to validate every message N decrypts, including rapid-fire and crossed-in-flight, not just the happy 1-at-a-time path.
