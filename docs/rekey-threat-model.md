# Re-key attack surface

What an attacker can do by claiming to be your friend and asking to re-key, and where the boundaries actually are. Grounded in the CLUTCH offer handler (`src/ui/photon_app.rs` `ClutchOfferReceived`) and the friend-history recovery path.

## The setup

A re-key is triggered implicitly: when a peer sends a `ClutchOffer` with keys **different** from the ones you last completed a ceremony with, you treat it as "peer lost their chains" and start a fresh ceremony — nuking your existing friendship chains and (as of history-recovery) zeroizing the history key. This is deliberate: it's what lets a friend who client-reset or got a new device re-establish a secure channel with **zero user friction**. That same friction-free path is the attack surface.

## What stops a pure spoofer (the primary boundary)

An attacker who is *not* cryptographically your friend cannot get anywhere near the re-key logic. An incoming offer passes three gates first:

1. **Signature.** The offer is a signed VSF frame; `read_verified` + `extract_signer_pubkey` reject anything not validly signed by the claimed `sender_pubkey`.
2. **Allowlist.** `sender_pubkey` must be in the answerable-pubkey set (`refresh_answerable_pubkeys` ← `Contact::answerable_pubkeys` = first-met device ∪ folded fleet members). An unknown key is dropped before the payload is even parsed.
3. **Device-identity match** (`ClutchOfferReceived`, the `expected != sender_pubkey` check): the offer must be signed by the contact's **first-met device pubkey** (`public_identity.key`) for that conversation. A mismatch is rejected outright.

Net: **you cannot spoof "I'm your friend, re-key me" without the friend's actual device private key.** There is no path where a stranger, or someone who merely knows your friend's handle, triggers a re-key. The conversation_token (`spaghettify(sorted identity seeds)`) is *not* relied on as the secret — it's derivable by anyone who knows both handles — the device-signature gate is the real boundary.

## What an attacker WITH a trusted key can do

The threat model that matters: the attacker holds a device key you already trust for this friend — via **device theft**, a **compromised device**, or (if/when the fleet-fold path widens gate 3 beyond first-met) a **fleet device that was removed but not yet revoked** on the friend's membership chain. Note that such an attacker is *already cryptographically your friend* — they can read and send messages as your friend regardless of re-key. Re-key grants them no new baseline access. What it adds:

1. **Denial of service / forced forgetting.** Each accepted re-key deletes your friendship chains and zeroizes the history key. An attacker can force this repeatedly, disrupting the live conversation and destroying forward-secret chain state on demand. (Plaintext message rows in rārangi survive — only chain state + the history key are destroyed.)

2. **Resource storm.** Re-keys involve ~550 KB offers + ~32 KB KEM responses each. Forced repetition wastes bandwidth and CPU (McEliece keygen). The post-completion cooldown added for the convergence bug damps accidental storms but is **not** an abuse limiter — a determined trusted attacker can re-key once per cooldown window indefinitely.

3. **History injection (the sharp one).** After forcing a re-key, the attacker becomes the peer you recover conversation history *from* (friend-assisted recovery, phase 1). They can serve **fabricated** history pages. Those rows are stored with `recovered = true` (friend-attested provenance — the honest hedge), but:
   - There is **no UI cue** distinguishing recovered rows from witnessed ones yet, so the user can't tell injected history from real.
   - Bounded by the merge dedup: recovered rows are keyed by `(timestamp, content)` and **local rows win on conflict**, so the attacker can *add* fabricated messages at new/empty timestamps but cannot *rewrite* history you already hold. The injection is "insert plausible lies into gaps," not "alter the record."

What the attacker **cannot** do via re-key: read *old* messages (forward-secret — the pre-re-key chain keys are already destroyed, so re-keying exposes nothing retroactively); impersonate you to third parties; or gain access they didn't already have from holding the trusted key.

## Revocation is not wired yet (the load-bearing caveat)

The whole "the defense against a compromised device is revocation" story is **aspirational today**. The chain primitives (`unbind_device`, `rotate_fleet_key`) exist and are server-verified, but device **REMOVE has no UI/flow** — `unbind_device` is called nowhere in the app, and `rotate_fleet_key` runs only on add/fold (`docs/device-lifecycle.md` §3: "the management UI is unbuilt"). So **you cannot currently revoke any device**, and a stolen trusted device stays trusted indefinitely. The only live defenses are the signature/device gate (keeps *strangers* out entirely) and the anti-storm cooldown (a convergence fix, not security).

Worse, there is a structural gap that will **survive** the arrival of device-remove unless fixed alongside it: **the first-met device (`public_identity`) is un-revocable by construction.** `Contact::answerable_pubkeys` unconditionally includes `public_identity.key` and then *adds* fleet members; the CLUTCH offer gate pins to `public_identity.key`; and `public_identity` is never re-pointed or dropped on fleet fold — revocation only rewrites the separate `fleet_members` set. So even once remove ships, removing the *first-met* device from the fleet chain will not stop it from passing the allowlist or the offer gate. Since the first-met device is the one most likely to be the stolen one, this is the case that matters most.

**Requirement for the device-remove milestone:** make trust respect the fold. Once a contact's fleet chain has been *successfully* folded, trust a device iff it is a current member — `public_identity` loses its unconditional pass if the folded chain excludes it. Fall back to `public_identity` only when no chain has been folded yet (bootstrap: a fresh contact is a de-facto single-member fleet). Gate this on a "have we ever folded a real chain for this contact" flag so a fold *failure* (network down) does not silently re-trust a revoked device. Without this, device-remove is incomplete for the highest-value theft case.

Note also: a fixed user secret does **not** help here. History injection presupposes device compromise, and any secret stored on the device is compromised with it. Only a secret that never touches the device (user-memorized, or hardware/PIPE) would resist it — the opposite of the friction-free handle-identity model, and out of scope until PIPE. The realistic levers are revocation-that-works and observability, not a portable secret.

### The MAC-in-ACK idea (and why it's moot)

There *is* a clean cryptographic defense against fabricated history — **if** a portable user-only secret existed. Each side would tag every message pair in its ACK: carry `spaghettify(message ‖ secret)` for both our outgoing and their incoming. On recovery/replay, recompute the tag over each served row and reject any that doesn't match. An attacker replaying or fabricating history can't produce the tag without the secret, so injection fails the check. But the secret doesn't exist (all roots are deterministic-from-handle or per-device — see above), so there's no MAC key and the scheme is unbuildable. And it's moot regardless: **fleet-allowlist update + key rotation on device-remove is the better answer** — it cuts the compromised device off at the source instead of detecting its lies after the fact. So revocation-that-works is the fix; the MAC-in-ACK is only what we'd reach for if we couldn't revoke.

### Fleet-list freshness on device-remove (requirement note)

When device management lands, the chain re-key allowlist must **honour the newest fleet list**: on receiving a buddy's fleet-membership list carrying a **newer eagle timestamp**, update our answerable/allowlist set from it (and re-key accordingly) rather than sticking with a stale membership. Otherwise a removed device stays trusted simply because we never refreshed. This is the mechanism that makes revocation actually propagate to the re-key gate — pair it with the `public_identity`-respects-fold requirement above.

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
- **Fleet-fold vs. first-met gate mismatch.** The allowlist honours the whole fleet, but the offer handler currently pins to the first-met device pubkey. If the offer gate is later widened to fleet devices (to let a friend's *new* device re-key), the attack surface widens to every fleet device — making fleet-membership revocation (rotate-on-remove) load-bearing for re-key safety. Decide this deliberately.

## One-line summary

A stranger cannot force a re-key — the device-signature gate stops them cold. An attacker who already holds a trusted device key (theft / compromise / stale fleet device) can force re-keys to DoS the channel, destroy forward-secret + history-key state, and inject fabricated history (add-only, tagged `recovered`, no UI cue yet). The exposure is entirely "someone who is already you," not "someone pretending to be you."
