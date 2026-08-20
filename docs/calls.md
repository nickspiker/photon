# Voice Calls — the concrete design

**Status:** v1 BUILT 2026-08-18 (audio, 1:1, fleet-native). Media engine, in-lane signaling, basket keys, ring/answer/hangup, recording-by-default all landed on the dev line; field verification pending a publish. This doc is the design the code follows (the lanes.md tradition); it supersedes any earlier call sketch.

Design settled in conversation with Nick. The through-line: **calls fall out of the architecture** — signaling is a conversation event, media is an ephemeral plane, ring/answer reuse the attention machinery, and nothing is a timer.

## The three planes

A call is three planes with different physics:

1. **Signaling — rides the lanes.** Offer/answer/decline/busy/hangup/taken are encrypted control ROWS (`CALL_PREFIX`, the probe/delete/attach convention) on the friendship lane. To every relay, queue, and wire observer a call is indistinguishable from a text message. What that inherits, for free: fold trust, receive-anywhere (every callee device decrypts the offer → every device rings, any can answer), dedup, retransmit, and — critically — the offer's lane key falls out of the decrypt as the basket's doomed egg. Code: `call/signal.rs`, dispatched in `conversation.rs` (RX) + `messaging.rs` (send-commit capture) + `call_ui.rs` (state machine).

2. **Media — an ephemeral UDP plane, deliberately NOT the ratchet.** Per-message ratcheting is wrong physics for 100 packets/second, and a call should be *more* forward-secret than messages. One non-VSF datagram in the whole system, stripped to a 5-byte clear header (2026-08-19): `[magic C7 | seq:4]` + XChaCha20-Poly1305 under the direction's step key, sealed payload = the BARE FEC symbol. Everything else was derivable or already proven by the AEAD and got deleted: step = seq/100, window = seq >> 1, symbol id = seq & 1, the ladder rung from the payload LENGTH (four rungs, four distinct window sizes), and direction + call identity live in the key (a wrong-direction or stale-call packet just fails to open). The magic byte sits in the HIGH half of ASCII — every other frame on the wire leads with plain ASCII ('R' for VSF, lowercase for PT), so one byte demuxes unambiguously. The recv worker checks the one magic byte before the entire parse ladder and routes matches raw to the engine — no PT ack, no StatusUpdate. Code: `call/packet.rs`, `call/engine.rs`, the fast path in `network/status.rs`.

3. **History — ordinary rows.** Missed/completed land as summary rows (retention-dialed like any content); a kept recording is an attachment-plane blob. The content plane never existed to store the media.

## Keys — the dozen-egg basket (`call/keys.rs`)

Nick: *"I don't ship one eggs, I ship a whole dozen."* A lone X25519 exchange would be one egg AND the only quantum-soft link in the system (everything else is symmetric/hash-based; a record-now-decrypt-later adversary gets nothing from the lanes). So the call secret derives from what both fleets already hold, three independent lineages deep:

```
call_secret = KDF("PHOTON_CALL_v1 call secret",
                  lane_root ‖ history_key ‖ lane_key@offer_position
                  ‖ call_id ‖ caller_nonce ‖ callee_nonce)
```

- `lane_root` — the era secret, ceremony-born, every device of both fleets.
- `history_key` — born at the same ceremony but OUTSIDE the ratchet (spaghettify over the pristine chains) — an independent lineage.
- `lane_key@offer_position` — **doomed material**: the ratchet destroys it as the lane advances past the offer, so it's the per-call forward-secrecy egg. The caller captures it at the send COMMIT (matched by content); each callee device at decrypt, pre-advance. The offer *names* its lane position; behind devices ratchet forward to it, never back.
- Nonces are public uniquifiers riding the offer/answer rows.

**Per-direction subkeys** (`"c>e"` / `"e>c"`) keep the two streams off one keystream; the packet nonce is the global sequence number (unique per key by construction — a step spans exactly `PACKETS_PER_STEP` seqs).

**Intra-call ratchet:** `ck_{i+1} = KDF(ck_i)` every 100 packets — **count-based, never a timer** — zeroizing the old step. A device compromised at minute 10 can't decrypt minute 1 of a recording. Teardown drops both `StepChain`s (their `Drop` zeroizes) — the call is cryptographically GONE. A packet from a destroyed step decrypts to silence, never a rewind. The KAT (`call_secret_known_answer`) is frozen from an independent computation — the mid-rollout compat tripwire.

## Codec + transport (`call/engine.rs`)

- **Opus RESTRICTED_LOWDELAY** (CELT-only, 2.5ms lookahead), **CBR on a channel-aware ladder** (2026-08-19): four rungs 16/32/64/128 kbps, every call starts at the floor and climbs on receive-side cleanliness (~1s clean = +1 rung), drops two rungs on a lost window — AIMD on edges, never timers. Within a rung the wire is constant-size CBR (the VBR packet-size side channel — phrase-spotting on encrypted VoIP — stays closed forever); a rung switch only tells an observer what the network already shows. Opus bandwidth follows the rate (WB at the floor thru fullband at the top), 10ms frames throughout. No SILK prediction, no in-band FEC/PLC — loss repair is principled, not psychoacoustic guesswork.
- **RaptorQ** over a tier-sized window — 4 frames (40ms) at the floor to amortize the per-packet fixed cost where bandwidth is scarcest, 2 frames (20ms) above for latency — (low rungs are genuinely cheap on the wire — no padding to the top rung's size). The symbol is exactly the window (windows are 8-aligned so raptorq's alignment changes nothing): 1 source + 1 repair = 2 packets per window, sealed payload = the bare symbol (window/esi/tier all derive — see the media-plane note above), and the window survives EITHER packet lost (p² loss odds — better than the old 3+2 spread AND ~3× fewer bytes). The receiver derives each window's slot walk + FEC geometry from the payload length, so mid-call rung switches decode seamlessly. The RX shape check (payload length must be exactly that rung's window) is load-bearing: raptorq panics on mis-sized symbols. Wire totals per direction: ~48 kbps floor / ~276 kbps top (originally ~218/~364); the fixed cost is 9 B/packet — 5 header + a 4-byte SRTP-32-style truncated Poly1305 tag (Nick's call: integrity-only online game at 2⁻³² per attempt against a call-lifetime key; the composition is hand-assembled RFC 8439 pinned bit-exact to the house AEAD by a KAT).
- **Jitter/order:** RX plays in strict window order; a hole with two completed windows behind it is declared lost and skipped — the dry playback queue renders the silence.
- **Egress from the main socket** via a dedicated awaited tokio forwarder (the peer's NAT knows that port; the polled request queues would add tens of ms).
- **Address-follows-auth:** an AEAD-valid packet from a new source re-points TX — NAT rebinds and (future) device handoff need no signaling; the AEAD *is* the authorization.
- **Relay-pipe media is deferred.** No direct path → a signaling-only, silent call, logged loudly; the transport-tier dot already tells the human they're on relay.

## Ring / answer — the attention machinery, inverted

- **Ring bypasses claim/attention suppression.** A call is the one always-ring event: `ring_alert` fires the platform notification + the relationship chirp (same song as their messages) on every device that DIRECTLY decrypted the offer.
- **Answering is taking the ball.** First answer wins (the caller adjudicates; later answerers get `Taken`); every other device stops ringing the instant it sees the sibling's answer row via merge.
- **No ring timer.** Ringing stops on local answer/decline, a sibling's answer, or the caller's hangup — **the caller's patience is the timeout**, and an unanswered hangup mints the missed-call row.
- **Ring requires DIRECT decrypt** — a signal arriving via sibling MERGE is history, not a doorbell: merge signals only ever STOP rings. This also kills the whole stale-offer-rings-days-later class (a woken device replays old signals and correctly rings for none).
- **Summary rows** are stamped `offer_osc + 1` on BOTH fleets (the offer row's wire timestamp is shared), so independently-minted copies merge-fold across every device.

## Recording — endpoint memory, not wire retention (`call/spool.rs`)

Nick: recording by default, each party chooses keep-or-delete after. The wire story is untouched (media step-keys still zeroize as the call runs; the ciphertext stream stays undecryptable forever). The recording is the endpoint keeping audio it already legitimately had:

- The engine spools the already-ENCODED frames both directions (~25MB/hour; "record" = not-discarding what Opus produced), each record sealed under a random per-call **spool key** held only in the `SpoolTicket`.
- At hangup → `Ended` phase, Keep/Delete bar. **Delete** = drop the ticket (key zeroizes, file is garbage — instant true crypto-shred; an app-crash-before-decision is equivalent, the key lived nowhere else). **Keep** = decrypt once into a segment-structured VSF container and store as a content-addressed blob + a fleet-internal attachment row (local insert + sibling push, never chain-transmitted — the friend's fleet keeps its own recording).
- **Container is segment-structured from day one** (`[dir | osc | len | opus]`, eagle-stamped): notes/transcription later are annotation layers on one timeline, and mid-call handoff produces a multi-segment call the fleet reassembles.
- **This is §14.8 doctrine applied to media**: destruction = key death, never "we deleted the recording we never made."

## Explicitly deferred (v1 gaps)

- **Mid-call handoff UX** — the keys are handoff-ready (any sibling derives the basket + joins the ratchet at the current step; address-follows-auth re-points the peer). The container is segment-ready. The UI + segment-reassembly + sibling blob-fetch of a kept call are the follow-up.
- **FCM doorbell cold-wake ring** — v1 rings presence-online devices only; a killed-service phone misses the call (the missed-call row still lands). Doorbell wake is the fast-follow.
- **Relay-pipe media**, **group calls**, **video track** (same container, another track), **local-only transcription**, **macOS VoiceProcessingIO**, **an in-house echo canceller** (echo layers 0-2 ship: headset-route bypass, platform AEC via VOICE_COMMUNICATION on Android, suppression duck fallback; layer 3 only if field logs demand it).

## Echo (the hard part — physics, not architecture)

Layered escape, cheapest first: **(0)** most calls are headsets — no acoustic path, bypass everything; **(1)** platform AEC where it exists — Android's `VOICE_COMMUNICATION` source engages the vendor canceller tuned for that exact device, better than anything generic we'd ship; **(2)** suppression-duck fallback (bare-ALSA Linux) — attenuate mic under far-end energy; **(3)** our own canceller, later, only if the field demands it, and then from our own log-corpus measurements (how the incumbents got good). The pipeline is **built for AEC before it has one**: every rendered frame lands in an eagle-stamped reference ring before the DAC (`platform/audio.rs`), so a future canceller gets an exact far-end reference — and we control both ends, so it's the decoded signal, not a guess at what a black-box stack played.
