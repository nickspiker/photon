# Voice Calls — the concrete design

**Status:** v1 BUILT 2026-08-18 (audio, 1:1, fleet-native). Media engine, in-lane signaling, basket keys, ring/answer/hangup, recording-by-default all landed on the dev line; field verification pending a publish. This doc is the design the code follows (the lanes.md tradition); it supersedes any earlier call sketch.

Design settled in conversation with Nick. The thru-line: **calls fall out of the architecture** — signaling is a conversation event, media is an ephemeral plane, ring/answer reuse the attention machinery, and nothing is a timer.

## The three planes

A call is three planes with different physics:

1. **Signaling — rides the lanes.** Offer/answer/decline/busy/hangup/taken are encrypted control ROWS (`CALL_PREFIX`, the probe/delete/attach convention) on the friendship lane. To every relay, queue, and wire observer a call is indistinguishable from a text message. What that inherits, for free: fold trust, receive-anywhere (every callee device decrypts the offer → every device rings, any can answer), dedup, retransmit, and — critically — the offer's lane key falls out of the decrypt as the basket's doomed egg. Code: `call/signal.rs`, dispatched in `conversation.rs` (RX) + `messaging.rs` (send-commit capture) + `call_ui.rs` (state machine).

2. **Media — an ephemeral UDP plane, deliberately NOT the ratchet.** Per-message ratcheting is wrong physics for 100 packets/second, and a call should be *more* forward-secret than messages. One non-VSF datagram in the whole system, stripped to a 5-byte clear header (2026-08-19): `[magic C7 | seq:4]` + XChaCha20-Poly1305 under the direction's step key, sealed payload = the BARE FEC symbol. Everything else was derivable or already proven by the AEAD and got deleted: step = seq/100, window = seq >> 1, symbol id = seq & 1, the ladder rung from the payload LENGTH (four rungs, four distinct window sizes), and direction + call identity live in the key (a wrong-direction or stale-call packet just fails to open). The magic byte sits in the HIGH half of ASCII — every other frame on the wire leads with plain ASCII ('R' for VSF, lowercase for PT), so one byte demuxes unambiguously. The recv worker checks the one magic byte before the entire parse ladder and routes matches raw to the engine — no PT ack, no StatusUpdate. Code: `call/packet.rs`, `call/engine.rs`, the fast path in `network/status.rs`.

3. **History — ONE row per wave (the wave card, 2026-09-09).** Every end edge mints the WAVE ROW at offer_osc+1 with a typed outcome (`WaveOutcome`: missed/busy/declined/answered/dropped, ranked so the sibling merge keeps the most-informed copy) and the live seconds; content is empty. A kept recording is an attachment-plane blob whose `call.audio` row REFERENCES the wave row (`RefKind::Wave`) and carries a one-gross envelope thumbnail; the renderer folds it into the wave row's card, so a wave is one event in the stream however many devices minted its pieces. The content plane never existed to store the media.

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

- **Opus RESTRICTED_LOWDELAY** (CELT-only, 2.5ms lookahead), **CBR on a channel-aware ladder** (2026-08-19): four rungs 16/32/64/128 kbps, every call starts at the floor and climbs on receive-side cleanliness (~1s clean = +1 rung), drops two rungs on a lost window — AIMD on edges, never timers. Within a rung the wire is constant-size CBR (the VBR packet-size side channel — phrase-spotting on encrypted VoIP — stays closed forever); a rung switch only tells an observer what the network already shows. Opus bandwidth follows the rate (WB at the floor thru fullband at the top), 5ms frames thruout (the 2026-09-08 latency flag day: 5ms frames + 10ms top-rung windows halved the fill wait, the batch wait, and the jitter quantum in one move; window geometry per rung = 8/4/2/2 frames). No SILK prediction, no in-band FEC/PLC — loss repair is principled, not psychoacoustic guesswork.
- **RaptorQ** over a tier-sized window — 4 frames (40ms) at the floor, 2 frames (20ms) above — with the repair PIGGYBACKED (2026-08-20): ONE datagram per window carries [ctrl][source(N)][repair(N−1)], so the packet rate halves, a window's two copies ride 20-40ms apart (a burst must kill two CONSECUTIVE datagrams to lose audio — strictly better than the old back-to-back pair), and clean-path latency is untouched (the source still ships the instant its window closes; only loss RECOVERY waits one window). The ctrl byte names both symbols' rungs because a rung switch mid-bundle makes length alone ambiguous — (low rungs are genuinely cheap on the wire — no padding to the top rung's size). The symbol is exactly the window (windows are 8-aligned so raptorq's alignment changes nothing): 1 source + 1 repair = 2 packets per window, sealed payload = the bare symbol (window/esi/tier all derive — see the media-plane note above), and the window survives EITHER packet lost (p² loss odds — better than the old 3+2 spread AND ~3× fewer bytes). The receiver derives each window's slot walk + FEC geometry from the payload length, so mid-call rung switches decode seamlessly. The RX shape check (payload length must be exactly that rung's window) is load-bearing: raptorq panics on mis-sized symbols. Wire totals per direction: ~46 kbps floor / ~281 kbps top on the meter with IPv4 (originally ~218/~364 before the strip series); slot headroom trimmed to hard-CBR+2; the fixed cost is 10 B/datagram — 5 header + 1 ctrl + a 4-byte SRTP-32-style truncated Poly1305 tag (Nick's call: integrity-only online game at 2⁻³² per attempt against a call-lifetime key; the composition is hand-assembled RFC 8439 pinned bit-exact to the house AEAD by a KAT).
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

**The card (`render.rs`, 2026-09-09).** The waveform IS the seek bar: ch0 (you) up from the centreline in your party colour, ch1 (them) down in theirs, played columns bright and the rest dim, a hairline playhead with elapsed/total in the header line. Tap the glyph to play/stop (or fetch the blob), press-and-drag the waveform to scrub (release = the seek edge, no timers), tap it to play from there. The envelope is computed at transcode (`record.rs`, every frame is decoded there anyway) in eighth-STOPS below full scale: the fine envelope (four buckets a second) rides the `PHCALL3` container header, the thumbnail (one gross of buckets per channel) rides the row so a sibling draws the shape before it holds the blob. Seek walks packet length prefixes to just before the target and primes the decoder — never a decode from zero. The stream filter pill in the top bar (all → waves → text) is one predicate in `chat_row_visible`, shared by the render and the jump walk.

Nick: recording by default, each party chooses keep-or-delete after. The wire story is untouched (media step-keys still zeroize as the call runs; the ciphertext stream stays undecryptable forever). The recording is the endpoint keeping audio it already legitimately had:

- The engine spools the already-ENCODED frames both directions (~25MB/hour; "record" = not-discarding what Opus produced), each record sealed under a random per-call **spool key** held only in the `SpoolTicket`.
- At hangup → `Ended` phase, Keep/Delete bar. **Delete** = drop the ticket (key zeroizes, file is garbage — instant true crypto-shred; an app-crash-before-decision is equivalent, the key lived nowhere else). **Keep** = decrypt once into a segment-structured VSF container and store as a content-addressed blob + a fleet-internal attachment row (local insert + sibling push, never chain-transmitted — the friend's fleet keeps its own recording).
- **Container is segment-structured from day one** (`[dir | osc | len | opus]`, eagle-stamped): notes/transcription later are annotation layers on one timeline, and mid-call handoff produces a multi-segment call the fleet reassembles.
- **This is §14.8 doctrine applied to media**: destruction = key death, never "we deleted the recording we never made."

## Explicitly deferred (v1 gaps)

- **Mid-call handoff UX** — the keys are handoff-ready (any sibling derives the basket + joins the ratchet at the current step; address-follows-auth re-points the peer). The container is segment-ready. The UI + segment-reassembly + sibling blob-fetch of a kept call are the follow-up.
- **FCM doorbell cold-wake ring** — v1 rings presence-online devices only; a killed-service phone misses the call (the missed-call row still lands). Doorbell wake is the fast-follow.
- **Relay-pipe media**, **group calls**, **video track** (same container, another track), **local-only transcription**, **macOS VoiceProcessingIO**, **an in-house echo canceller** — SHIPPED 2026-09-08: chirp-seeded NLMS (call/nlms.rs), born converged from the connect probe's measured impulse response, adaptation gated on far-talks-alone, duck demoted to residual suppressor while armed. RLS rejected on purpose (O(L²), numerically fragile, and its convergence advantage is void when the probe pre-converges the filter).

## Echo (the hard part — physics, not architecture)

Layered escape, cheapest first: **(0)** most calls are headsets — no acoustic path, bypass everything; **(1)** platform AEC where it exists — Android's `VOICE_COMMUNICATION` source engages the vendor canceller tuned for that exact device, better than anything generic we'd ship; **(2)** suppression-duck fallback (bare-ALSA Linux) — attenuate mic under far-end energy; **(3)** our own canceller, later, only if the field demands it, and then from our own log-corpus measurements (how the incumbents got good). The pipeline is **built for AEC before it has one**: every rendered frame lands in an eagle-stamped reference ring before the DAC (`platform/audio.rs`), so a future canceller gets an exact far-end reference — and we control both ends, so it's the decoded signal, not a guess at what a black-box stack played.

## Fleet lifecycle (2026-09-08 — after the sibling-hangup incident)

Three ownership rules, each convicted or confirmed in the field:

1. **Ring wants breadth, replies want precision.** The offer's express copy fans to every fold-trusted device endpoint of the callee (the ding beats replication); every other signal targets the ONE peer device driving the call — its express source + its own endpoint addresses. Multiple addresses of one device is a race; multiple devices was the 2026-09-08 bug (Brittany's answer reached Nick's non-calling sibling, whose crash-recovery branch hung the call up 107ms after answer). `validated_path` is per-CONTACT, so it is only ever the no-better-knowledge fallback.
2. **Your fleet's state is announced by your own device, never the peer.** The answer row replicates fleet-wide: siblings stop their rings and light the presence chip ("wave in progress on <machine>") off their OWN fleet's row. Siblings observe, never destroy — the loud-kill on a strayed answer is licensed by `dialed_call_ids` (only the device that dialed may kill), which is also precondition zero for join/switch-mid-call later.
3. **Only the far end arbitrates races.** Two devices answering in the same instant can't self-order; the caller picks the first and `Taken`s the loser.

**Ring lease (2026-09-08).** The one place edges-not-timers can't hold: "stop ringing" on a NON-answering device has no delivered edge (its stop rode fleet replication, which drops — the desktop+mac forever-ring). So the ring is a LEASE on the offer heartbeat, not a UI timer: the caller re-expresses the offer every ~1s while Outgoing, and a callee whose Ringing call sees no beat for ~3s lapses it silently. The beat stops the instant the caller leaves Outgoing — answered by ANY of the callee's devices, or hung up — so the offer stream drying up is the universal, loss-proof stop that reaches every ringing device at once WITHOUT any callee-side replication. Instant stops (Hangup/Decline/sibling-answer) still fire on their edges; the lease is the floor under them. The lapse mints NO row: the caller signs the missed-wave record (its Outgoing-hangup MissedCallRow), never the receiver (device sovereignty).

**Device-named signals.** Offer and Answer carry the originating/answering device pubkey as an optional 4th wire field (old rows parse with `None`; a garbled field degrades to `None`, never a dropped signal). It is routing + presence info gated thru `knows_device` — authentication stays with the AEAD/lane. The callee routes its answer at the ORIGIN device's freshest endpoints instead of the offer's possibly-stale source address.

**Media liveness.** Packet arrival is the event stream; the drought tick is a measurement cadence on it (the learner-tick/RTO law, never UI timing). 5s of receive drought → "reconnecting" + express `Anchor` re-fires (an authenticated anchor's SOURCE address re-points the far engine's TX — the signal-plane heal for what address-follows-auth can't fix alone); 30s → honest teardown with a "dropped" summary row (the durable Hangup row converges a far end alive behind a dead path, and tombstones the chip). Mute transmits ZEROS, not absence: the CBR cadence never breaks, so a muted stretch is invisible on the wire, holds NAT pinholes, and can never read as a drought. Known gap: a simultaneous both-sides address change still needs a relay-carried anchor bearing the reflexive address — the direct anchor only heals paths where one side's address survived.
