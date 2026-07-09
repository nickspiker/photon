# Re-key attack surface

What an attacker can do by claiming to be your friend and asking to re-key, and where the boundaries actually are. Grounded in the CLUTCH offer handler (`src/ui/photon_app.rs` `ClutchOfferReceived`) and the friend-history recovery path.

## The setup

A re-key is triggered implicitly: when a peer sends a `ClutchOffer` with keys **different** from the ones you last completed a ceremony with, you treat it as "peer lost their chains" and start a fresh ceremony — nuking your existing friendship chains and (as of history-recovery) zeroizing the history key. This is deliberate: it's what lets a friend who client-reset or got a new device re-establish a secure channel with **zero user friction**. That same friction-free path is the attack surface.

## What stops a pure spoofer (the primary boundary)

An attacker who is *not* cryptographically your friend cannot get anywhere near the re-key logic. An incoming offer passes three gates first:

1. **Signature.** The offer is a signed VSF frame; `read_verified` + `extract_signer_pubkey` reject anything not validly signed by the claimed `sender_pubkey`.
2. **Allowlist.** `sender_pubkey` must be in the answerable-pubkey set (`refresh_answerable_pubkeys` ← `Contact::answerable_pubkeys`). Fold-respecting: post-fold that's exactly the current folded members; pre-fold it's first-met ∪ cached members. An unknown key is dropped before the payload is even parsed.
3. **Trusted-device match** (`ClutchOfferReceived`, the `knows_device(sender_pubkey)` check): the offer must be signed by a device this contact currently trusts. Post-fold that's a current folded member (so a *removed* device is rejected and a friend's *new* device is accepted); pre-fold it's the first-met device. A mismatch is dropped.

Net: **you cannot spoof "I'm your friend, re-key me" without the friend's actual device private key.** There is no path where a stranger, or someone who merely knows your friend's handle, triggers a re-key. The conversation_token (`spaghettify(sorted identity seeds)`) is *not* relied on as the secret — it's derivable by anyone who knows both handles — the device-signature gate is the real boundary.

## What an attacker WITH a trusted key can do

The threat model that matters: the attacker holds a device key you already trust for this friend — via **device theft** or a **compromised device**. (A *removed* fleet device is no longer such a key: gate 3 is now fold-respecting, so once the friend's chain folds without it, it's rejected — see below.) Note that such an attacker is *already cryptographically your friend* — they can read and send messages as your friend regardless of re-key. Re-key grants them no new baseline access. What it adds:

1. **Denial of service / forced forgetting.** Each accepted re-key deletes your friendship chains and zeroizes the history key. An attacker can force this repeatedly, disrupting the live conversation and destroying forward-secret chain state on demand. (Plaintext message rows in rārangi survive — only chain state + the history key are destroyed.)

2. **Resource storm.** Re-keys involve ~550 KB offers + ~32 KB KEM responses each. Forced repetition wastes bandwidth and CPU (McEliece keygen). The post-completion cooldown added for the convergence bug damps accidental storms but is **not** an abuse limiter — a determined trusted attacker can re-key once per cooldown window indefinitely.

3. **History injection (the sharp one).** After forcing a re-key, the attacker becomes the peer you recover conversation history *from* (friend-assisted recovery, phase 1). They can serve **fabricated** history pages. Those rows are stored with `recovered = true` (friend-attested provenance — the honest hedge), but:
   - There is **no UI cue** distinguishing recovered rows from witnessed ones yet, so the user can't tell injected history from real.
   - Bounded by the merge dedup: recovered rows are keyed by `(timestamp, content)` and **local rows win on conflict**, so the attacker can *add* fabricated messages at new/empty timestamps but cannot *rewrite* history you already hold. The injection is "insert plausible lies into gaps," not "alter the record."

What the attacker **cannot** do via re-key: read *old* messages (forward-secret — the pre-re-key chain keys are already destroyed, so re-keying exposes nothing retroactively); impersonate you to third parties; or gain access they didn't already have from holding the trusted key.

## Revocation is wired (2026-07-09)

Revocation now works end to end. Device **REMOVE** shipped: Settings → Fleet drives `unbind_device` + mandatory `rotate_fleet_key(survivors)` (excluding the removed device, so it can't decrypt the new epoch), and any remaining member can sign the Remove. The removed device's next attest hits "not in the fleet", and it can shed the identity via the Security "Shred" / JOIN "Start fresh" clean so the hardware is reusable.

The first-met un-revocability gap is closed: `knows_device`/`answerable_pubkeys` are **fold-respecting**. Once a contact's chain has *successfully* folded at least once (`fleet_folded_once`), trust a device iff it is a current folded member — `public_identity` loses its unconditional pass if the fold excluded it (the removed-first-met case, most likely to be the stolen device). Pre-fold is bootstrap (`public_identity` ∪ cached members). A fold *failure* never arms the flag, so a network outage never silently re-trusts a revoked device or flips a healthy contact to trust-nobody. The three CLUTCH gates (offer/KEM/complete) call `knows_device` rather than pinning to first-met — which both enables revocation and lets a friend's current device CLUTCH.

Freshness is monotonic: the contact-fleet fold now carries the chain-tip eagle time, and the drain adopts a fold only if its tip is ≥ the last adopted one, so a stale R2 eventual-consistency read can't resurrect a removed device. On a membership shrink the answerable set is reseeded (removed device dropped) before persist, so an in-flight pong the same tick already sees it gone. The folded set + arm flag + tip ts persist, so a restart resumes members-only trust with no bootstrap-regression window.

Residual live defenses that remain relevant: the signature/device gate (keeps *strangers* out entirely) and the post-completion re-key cooldown (a convergence fix, not security).

Note also: a fixed user secret does **not** help here. History injection presupposes device compromise, and any secret stored on the device is compromised with it. Only a secret that never touches the device (user-memorized, or hardware/PIPE) would resist it — the opposite of the friction-free handle-identity model, and out of scope until PIPE. The realistic levers are revocation-that-works and observability, not a portable secret.

### The MAC-in-ACK idea (and why it's moot)

There *is* a clean cryptographic defense against fabricated history — **if** a portable user-only secret existed. Each side would tag every message pair in its ACK: carry `spaghettify(message ‖ secret)` for both our outgoing and their incoming. On recovery/replay, recompute the tag over each served row and reject any that doesn't match. An attacker replaying or fabricating history can't produce the tag without the secret, so injection fails the check. But the secret doesn't exist (all roots are deterministic-from-handle or per-device — see above), so there's no MAC key and the scheme is unbuildable. And it's moot regardless: **fleet-allowlist update + key rotation on device-remove is the better answer** — it cuts the compromised device off at the source instead of detecting its lies after the fact. So revocation-that-works is the fix; the MAC-in-ACK is only what we'd reach for if we couldn't revoke.

### Fleet-list freshness on device-remove (SHIPPED)

The chain re-key allowlist **honours the newest fleet list**: the contact-fleet fold carries the chain-tip eagle time (`current_members_with_ts`), and the drain adopts a fold only if its tip is ≥ the last adopted one, so a stale read can't resurrect a removed device. On a membership shrink it reseeds the answerable set (removed device dropped) before persist. This is what propagates a friend's removal to our re-key gate — paired with the fold-respecting `public_identity` rule above, a removed device drops out of `answerable_pubkeys` and fails `knows_device` within a refresh cycle.

## Present mitigations vs. gaps

Present:
- Signature + allowlist + device-identity gate (the real boundary; a keyless spoofer never reaches re-key).
- `completed_their_hqc_prefix` — a re-offer with the **same** keys is ignored, so replaying a captured offer can't force a re-key; only genuinely new keys do.
- Post-completion re-key cooldown — ignores re-key offers within ~10 s of completion (anti-storm / anti-race, not anti-abuse).
- Recovery merge keeps local rows on conflict + tags recovered rows.

Gaps (future hardening, in rough priority):
- **No re-key rate limit / abuse signal.** A trusted-key attacker can force a re-key every cooldown window forever. Worth: a per-contact re-key counter with a warn threshold, and backing off / requiring user confirmation after N re-keys in a window.
- **No user-visible re-key notification.** A forced re-key (and the chain/history destruction it causes) is silent. A "your secure channel with X was re-established" banner would make DoS and history-injection at least *observable*.
- **No recovered-row UI cue.** Until recovered rows are visually distinguished, injected history is indistinguishable from witnessed history to the user. (The data model already carries the flag; only the UI is missing.)
- **Re-key is unauthenticated as to cause.** "Different keys ⇒ they lost their chains" is an assumption, not a proof. There's no challenge that the peer actually lost state. A trusted device can assert a re-key at will. A heavier design (e.g. requiring the re-keying side to prove loss, or a user-confirmed re-pair for a device that's *not* the first-met one) would trade friction for abuse-resistance.
- **Fleet-fold gate is now load-bearing (decided, shipped).** The offer/KEM/complete gates were widened from the first-met pin to `knows_device` (fold-respecting), so a friend's *new* device can re-key and a *removed* device cannot. This deliberately makes fleet-membership revocation (rotate-on-remove + monotonic fold adoption) load-bearing for re-key safety — which is the point: revocation is the primary defense against a compromised device.

## One-line summary

A stranger cannot force a re-key — the device-signature gate stops them cold. An attacker who already holds a trusted device key (theft / compromise / stale fleet device) can force re-keys to DoS the channel, destroy forward-secret + history-key state, and inject fabricated history (add-only, tagged `recovered`, no UI cue yet). The exposure is entirely "someone who is already you," not "someone pretending to be you."
