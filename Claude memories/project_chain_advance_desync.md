---
name: project_chain_advance_desync
description: "CLUTCH completes + first message decrypts, but the message-chain ratchet diverges by message 2 — second decrypt yields garbage (wrong derived key)"
metadata: 
  node_type: memory
  type: project
  originSessionId: 578828f3-1ff5-4749-8ef6-b66268a7aa19
---

Diagnosed 2026-06-27 from Nick's log (desktop "Nick" ↔ phone "Jennifer"). DISTINCT from [[project_clutch_offer_deadlock]] (that = Pending-forever). Here CLUTCH reaches Complete and messages flow, but the double-ratchet message chain desyncs after the FIRST message.

**Symptom:** "chain isn't working right." First message between Nick and Jennifer decrypts fine; the second decrypts to garbage.

**Evidence (Nick's log, both `CHAIN DECRYPT` lines):**
- Msg 1 (~line 7144-7147): `key=30c73467 salt=95672ec4` → raw plaintext bytes START `[40,100,51,7,109,101,115,115,97,103,101,58,120,51,...]` = ASCII `(d3·message:x3` = a VALID VSF message header. Decrypt good. Followed by `CHAT: Updated hash chain for Jennifer - msg_hp=e54da7c4...` and earlier `CHAT: Chain advanced for Jennifer (ACK verified)`.
- Msg 2 (~line 7882-7883): `key=817e4e83 salt=27497347` → raw plaintext `[105,28,92,253,89,33,247,...]` = RANDOM NOISE, not the `(d3message` header. Garbage = WRONG decryption key.

So the ratchet ADVANCED (key changed 30c73467 → 817e4e83, "Chain advanced ... ACK verified") but the next-key Nick derived ≠ the key Jennifer encrypted msg 2 with. The two sides' key/salt advancement diverged after msg 1. Classic double-ratchet desync — the salt/weave/key derivation in the chain-advance step is not staying in lockstep across the two devices.

**Also seen:** a FLOOD of repeated `clutch_complete` proof packets (Nick's log ~7368-7943, dozens) — the ClutchComplete retransmit budget firing repeatedly, suggesting completion isn't being settled/acked symmetrically (may or may not be related to the chain desync; note both).

**Where to look:** the chain ratchet advance + key/salt derivation. Relevant types/fields: FriendshipChains (src/types/friendship.rs) — last_sent_hash, last_received_hash, last_sent_weave, last_received_weave, last_incorporated_hp, last_plaintext (v4, "needed for salt derivation after restart"), last_received_time. The "CHAIN DECRYPT: ...key=...salt=..." log and "CHAT: Chain advanced / Updated hash chain" are the instrumentation points. The salt derivation depends on last_plaintext — a prime suspect: if the two sides disagree on what the previous plaintext was (e.g. one stored the encrypted/binary form, one the decoded form), the salt diverges on msg 2. Nick's log line 3366/3368 shows last_plaintext stored as binary-ish `x4⦉310⦊"<310 bytes binary>..."` AND an empty `x3⦉0⦊""` — worth checking the last_plaintext capture is identical on both ends.

**Blocker for full diagnosis:** the phone (Jennifer) is a RELEASE build — its photon log is in app-private internal storage, not pullable via `run-as` (not debuggable), and photon's `crate::log` does NOT go to logcat. So only Nick's half is visible. To get both-sides key/salt would need a debuggable Android build or routing photon logs to logcat/a pullable path.

**UPDATE 2026-06-28 — partial fix landed + full design written.**

The weave-snapshot fix (commit 34fc92d): freeze the incorporated peer-plaintext on each PendingMessage at send time; process_ack advances with THAT, not order-dependent "latest". Confirmed live to ADVANCE the failure from "msg 2 garbage" → "msg 3 garbage" (msg 2 now decrypts, weave correct). Real progress, not a full fix.

Two residual bugs found from live BOTH-SIDES logs (now have phone logcat via dev build, tag `photon`):
1. Weave selection still implicit/"latest" — sides disagree under messages-in-flight.
2. **Advance is ACK-timing-gated → stale-key reuse.** Nick reused the SAME key+salt (ef1f5e04/561e2363) for two consecutive RECEIVED messages — chain didn't advance between them, so msg N+1 decrypted with msg N's key → garbage. Trigger: multiple peer messages arriving before the chain advances (rapid/crossed-in-flight); the happy 1-at-a-time path works.

Derivation contract (src/crypto/chain.rs): `derive_fresh_link` mixes DOMAIN+eagle_time+our_plaintext+chain_portion+their_plaintext; `derive_salt` mixes DOMAIN+prev_plaintext+last-12-links. Both must agree on their_plaintext AND chain position.

**NAME: the construction is "the braid"** (not a ratchet — a ratchet only goes forward; the braid reaches back into history and cross-weaves the peer's strand = bidirectional cross-entropy). Just "braid", no number — the window size (currently 256) is a tunable PARAMETER, not part of the name. The reach is what makes it not-a-double-ratchet: a double ratchet weaves depth-1 (the immediately-prior step); the braid weaves depth-1 per message but the source K is chosen at random from a WINDOW of the last ~256 messages, so which prior secret mixes is unpredictable (window, not count). "Confluent" = a described PROPERTY (explicit-hash refs → any delivery order converges), not part of the name.

**THE FIX — IMPLEMENTED 2026-06-28, commit 9bf1193** (was designed in [docs/chain-explicit-weave.md](/mnt/Octopus/Code/photon/docs/chain-explicit-weave.md); ALL chain+friendship tests pass, desktop dev build links+signs clean; still needs live Nick↔phone validation, rapid-fire + crossed). Two layers, both landed:

Layer 2 (the braid): each step weaves TWO distinct prior PEER messages (0→none, 1→one, ≥2→two distinct — the defining property). Ingredient = the message's x-text (`content`), recoverable identically both sides from the rārangi message DB (NO separate ring — the design's "hash-indexed ring of 256" was replaced by the message DB tail). Reference = the woven message's **eagle_time** on the wire as `e6` VSF values (NOT msg_hp — eagle_time is provably unique within one device's 704ps-tick stream). Sender selects via `csprng gen_range(0..window)` over last ≤256 incoming msgs (never modulo). `derive_fresh_link` reordered: peer entropy FIRST, each strand length-prefixed, strands sorted by eagle_time so both peers frame identically. Sender resolves strands from its INCOMING msgs; receiver resolves the SAME from its OUTGOING msgs (the peer wove messages we authored) → byte-identical → lockstep. **Derivation changed → old chains incompatible (dev: nuke+rekey).**

Layer 1 (strict ordering): `verify_chain_link` is now HARD — an "ahead" message (predecessor unseen) is buffered (gap_buffer wired at last; keyed on awaited `prev_msg_hp` since msg_hp is unknown pre-decrypt) and skipped, not soft-decrypted at the wrong link. After a good decrypt+advance, `take_buffered_for(msg_hp)` drains now-contiguous msgs onto a front-drained replay queue in photon_app's receive loop (cascades). Sender-advance-on-ACK kept (load-bearing for reliability + CLUTCH zeroize). Message rows now keyed by eagle_time (monotonic), content_hash stored.

Known gap: adversarial same-tick eagle_time collision (two devices, same identity) has no content_hash tiebreak on the wire yet — receive-side resolution just takes first match. `incorporated_hp`/`update_received_for_mixing`/`last_incorporated_hp` legacy machinery still present (implicit-ACK) but no longer the weave reference.

**UPDATE 2026-07-23 — LIVE TWO-SIDED REPRO post-9bf1193, GROUND-TRUTH-VERIFIED (pid probe): the SIBLING 1:1 desyncs right after mutual weave. Do NOT let this state get cleared.**

Setting: Nick fleet after device-ADD — desktop 90e571bf (device name "FlakyPositive", sibling-pid 1af83ffd → pseudonym "BarkCook") ↔ phone 1be949c1 ("LotteryStandard", sibling-pid d48d972b → "TheoryConvertible"). Logs via `photonlog --pull --handle <handle>`.
**Ground truth that took three passes to get right (see [[reference-log-pull]] naming traps):** ceremony/punch log lines print `fp(contact.handle_proof)` — and every SIBLING contact carries OUR OWN handle_proof (7ff3835f = Nick's hp), so "offer/proof with 7ff3835f" on BOTH devices = the SIBLING WEAVE, not a friend. The braid-in RAN and COMPLETED (both sides: probes RX-proven, "chain woven — end-to-end verified" ~15:33:03-06); [[project_keyring_design]]'s "braid-in never fires" is NOT the bug. The three PUNCH-stuck contacts (hps 633219a1/5fb63507/aefd768a) are real friends, offline — expected.
The bug: immediately after the CROSS-PROBE race (phone probe 15:33:03.4, desktop probe 15:33:04.1, crossing in flight) the sibling 1:1 chain deadlocked:
- Desktop head = 0a3a8ba3 (phone's probe); incoming from phone claims prev=9a792020 → Layer-1 "Hash chain gap — buffering (ahead of us)" forever.
- Phone head = 9a792020 ALREADY at 15:33:03.5, BEFORE it logged decrypting the desktop's probe at 15:33:06.5 (dual-path LAN+relay double-delivery advancing early is the suspect); incoming from desktop claims prev=f20e656d → mirrored buffering. Three head claims in a two-message-old conversation.
- The gap never drains: desktop retransmits its probe (eagle 2555318968713282048) forever; phone skips each as "duplicate — no stored ack_hash (pre-fix message or outgoing)" so the re-ACK self-heal is structurally blocked for probe/outgoing-shaped rows; an ACK for that eagle also hit the phone and failed ("no matching pending message") — relay-echo/misrouting suspected.
- Because the seal (first bidirectional ACK) never lands, the fleet page shows the weave stuck ("LotteryStandard — weaving") even tho the ceremony crypto completed.
- Noise: devices receive relay echoes of their OWN offers/completes, dropped as "untrusted/removed device <self>".
This IS the chain-advance-desync family, first two-sided simultaneous capture. Suspects: cross-probe incorporation ordering under crossing sends + dual-path double-delivery advancing one head early + the no-ack_hash duplicate seam. FIX THIS NEXT — it gates fleet-sync/message-recovery testing and the notification build.
17:00 same day: the gap SURVIVES app restarts and the v0.42.3/4 builds (desktop still buffering the same expected-0a3a8ba3/got-9a792020 pair; phone side quiet this session) — the forked heads are in PERSISTED chain state, so no restart will ever heal it; the fix needs either a chain-state reconcile or (dev) a sibling re-key.
ALSO OBSERVED 2026-07-23 17:00 (separate anomaly, park): BOTH Nick devices receive a ClutchComplete every ~10s for token 4d56c97a (proof f0f673b5) they cannot resolve, from a device in friend f2103dc8's fleet (same Verizon /64, different interface id) — likely the FRIEND's own fleet-internal weave proof mis-addressed to Nick (their side runs the same rebroadcast machinery; Nick can't ACK an unknown token, so it never stops), or a pid/token derivation mismatch. Needs THAT identity's logs; "GOSSIP/harvest: adopted a stalled contact's address" ×5 on the phone at the same moment is a suspicious neighbour (cross-contact address pollution?).

UPDATE 2026-07-25: several seams from the 07-23 repro are now FIXED in code (E2E re-verify pending on v48): probe rows persist WITH ack_hash (hidden rarangi row → duplicates re-ACK, the structural block on the seal is gone); real decrypted messages seal the chain directly (reload deadlock closed); fork-streak auto re-key (streak 3, friends) + sibling chain-reset machinery exist; fleet delivery no longer depends on presence (unconditional sibling push @ d73c223) and unmatched pongs count as liveness (@ a376a17). The persisted forked sibling heads from 07-23 may still need the reset to fire or a dev re-key. Sarah↔Nick adds the ADOPTION-COOLDOWN case: her pre-v47 build re-offers fresh keys ~25s forever when she can't hear replies (one-way reachability — replies went to her unroutable home-LAN addr; relay copy undrained); our side holds its round 60s per adoption. Her update is the prerequisite for that pair to heal.
