---
name: project-pairing-v2
description: "Pairing v2 words-first BUILT + DEPLOYED 2026-07-13 — bindreq registry live on fgtw.org, consent-egg chain, matcher + two-phase UI; 3 live tests green, 2-device on-screen E2E pending"
metadata: 
  node_type: memory
  type: project
  originSessionId: fd164ac0-4225-4073-99e9-66d5cd5179bb
---

Pairing v2 REDESIGNED 2026-07-13, same-day rewrite of the lock-word-first draft (docs/pairing-v2.md is canonical; opens with the [[project-device-sovereignty]] manifesto).
Key reframe: words/NFC/BLE are three SELECTORS on one candidate-set machine; the registry authenticates candidates, the selector picks, the WAN never selects.
v1's real flaws (the "words were WAN data" framing was retired as unfair — the v1 match was exact and sound): clobberable single pair slot (remote silent DoS), 23-word burden as a consequence of WAN candidate delivery, ownership-by-listing squat, no device consent on Add.

Decided design (all confirmed by user):
- Binding request registry: keyed (hp, device_pubkey) set — write gated on device sig + identity co-sig (checked vs chain genesis identity_pubkey), read member-gated (inbox_drain gate), author-withdrawn or 5-min-lapsed, worker NEVER consumes; hub pair_evt gains "request" kind, "matched" retires.
- Masked words: pair_words(device_pubkey XOR blake3::derive_key("photon pair words v1", identity_seed)), fixed 23 words, NO checksum (live matcher subsumes typo detection: prefix-match typed entry against expected strings per candidate, divergence flags the exact word).
- Add carries consent egg: consent_t + consent_sig (the request's device signature) bound into signing_bytes genesis-style; fold rejects |eagle_time − consent_t| > 1h. Conscription structurally impossible.
- Remove = self-signed departure ONLY, flipped NOW ahead of the withholding layer (user chose manifesto-pure eyes-open): no eviction of a lost device until S/friendship re-key lands; interim withholding = fleet-key subset rotation; remove-other UI retires; test pruning = re-genesis.
- Two-phase everywhere: bind → green lamp → human confirm → rotate. Wrong bind = keyless permanent testimony entry + rotate-around + local tombstone (no unbind exists).
- PROXIMITY-POPULATES-THE-LIST rule (2026-07-14, load-bearing): the old device's tappable candidate list is populated ONLY by proximity (BLE announce heard, later NFC tap), NEVER by the WAN binding-request registry. Reason: a remote attacker holding the handle can FLOOD the identity-gated registry with requests → listing registry entries as tap targets fills the user's finger-reach with decoys. Registry = sync only (carries the consent sig a tap binds with); proximity = the thing a remote attacker can't fake. No BLE/NFC → user types the words (reading words off the physical screen IS the proximity check). Words path auto-rotates (typed key = confirmation); tap/proximity path takes the two-phase "did it turn green?" confirm. BUILT: AddCandidate.heard_ble filters both render + tap dispatch.
- Worker: bindreq_put/list/withdraw replace pair/pack slots; device→owner index claims ONLY from consent-carrying ops, releases on self-departure/supersession.
- Flag-day migration: consent-less chains don't fold; wipe + re-genesis the ~5 test devices.

BUILT + DEPLOYED same day (2026-07-13): fgtw crate (SIGNING_DOMAIN bumped v0→v1, consent egg in signing_bytes genesis-style, 1h window, RemoveNotSelfSigned, BindRequest::verify, masked_device_words; 35 tests), worker (bindreq_put/list/withdraw — keyed set, dual-sig write gate vs genesis identity_pubkey, member-gated read, lapsed-GC, empty list = not_found frame since a zero-field VSF section doesn't parse; hub "request" kind; pair/pack handlers deleted), photon (AddCandidate matcher with per-keystroke token prefix-match + divergence flagging, auto-bind on full match, "It's in — finish" green-confirm gating the rotate, join posts/withdraws bindreq + 60×2s key wait spanning the human confirm, remove-other UI + RemoveDeviceUpdate deleted).
FLAG-DAY implemented as LAZY supersession (no manual wipe, no admin token needed): stored chain that fails parse OR fold under v1 = dead-format = treated as absent by worker handle_fleet_op and client ensure_member's genesis path only — old devices re-attest straight into fresh genesis; remove those two branches later.
Live tests green vs deployed worker: device_add round trip (put→list→match→bind→rotate→recover→withdraw→member-gate), fanout rotation with self-departure, bind_attempt alert.
FIRST ON-DEVICE RUN (2026-07-13/14, fedora↔phone) found + fixed: (1) join loop camped 60×2s on the fleet key, which is gated behind the sponsor's green-confirm → circular wait — Joined now fires the moment membership folds (leaving the words screen IS the green), key follows via event sync; (2) recover_or_establish let the freshly-bound device ROTATE ITSELF the key (any member may rotate), silently voiding the two-phase gate — it now establishes ONLY when no fanout exists; fanout-without-our-wrap = wait for the sponsor's confirm; (3) worker broadcasts "fleet" on fanout_put so the joiner's key sync fires seconds after the confirm press; (4) old device returns to Settings(Fleet) on finish/cancel/Escape (was Ready → looked stuck); (5) wake_at gains a 2 Hz drain arm while either pairing rx channel is live (mpsc results otherwise sat undrained with hands off the keyboard).
TO DO: re-run two-device ceremony (desktop + Android via scripts/android/dev-adb.sh), NFC then BLE transports.
Carried over, built: current_members_verified + genesis-every-fetch join loop; milestone-A shadow beacon (pairing_beacon.rs, PhotonBeacon.kt, bluer scan) kept as the future BLE transport's radio path; lock_word/word_mac stay for BLE's selector later.
Related: [[project-device-sovereignty]] (governing rule), [[project-keyring-design]] (v1 this supersedes), [[project-rekey-attack-surface]] (withholding layer home), [[project-device-loaners]] (removal model source).
