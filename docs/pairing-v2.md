# Pairing v2 — words, bilateral bind, sovereign records

Status: BUILT 2026-07-13 (same day as the redesign) — fgtw crate (consent-egg fold, BindRequest, masked codec), worker (bindreq registry, flag-day supersession, deployed to fgtw.org), photon (live matcher + two-phase confirm on the old device, masked-words join + withdraw-on-green on the new device, remove-other UI retired).
All three live tests pass against the deployed worker (`live_device_add_round_trip`, `live_fanout_rotation_round_trip`, `live_bind_attempt_alert`); the two-device on-screen ceremony is still to be run by hand.
Flag-day note: implemented as LAZY supersession rather than a manual wipe — a stored chain that no longer parses/folds under v1 rules is dead-format state, treated as absent by `handle_fleet_op` and by `ensure_member`'s genesis path, so each old device re-attests straight into a fresh genesis. Remove the two flag-day branches once no v0 chain can plausibly remain.
Words are the first transport; NFC and BLE follow as transport swaps into the same machine.
Carries over from the earlier draft: `current_members_verified` + the join loop's genesis-on-every-fetch fix (built), the milestone-A shadow beacon (Android advertise/scan JNI, bluer Linux scan, the `pairing_beacon.rs` seam — built, kept as the future BLE transport's radio path), and `lock_word`/`word_mac`/beacon codec in `fgtw::pair` (built, demoted to the BLE transport section below).

## The manifesto

**I own myself, my devices own themselves, and I own my devices. Everything else flows from that.**

- *I own myself*: genesis is co-signed by `Ed25519(identity_seed)` — no vendor, no reset authority, custodian-gated supersession as the only pre-consented override.
- *My devices own themselves*: every record about a device is signed by that device and mutable only by that device; everyone else verifies or withholds.
- *I own my devices*: adds are sponsored by a member device and bound to my handle; a foreign handle cannot displace my ownership of record.

The rule that decides every lifecycle question: **the subject signs; others verify or withhold. Pending records expire; completed records are permanent testimony. Security is ostracism, never erasure.**
No verb exists by which one party edits another's standing — only verbs of self (request, consent, resign) and verbs of group (verify, include in the next key, exclude from the next key).
The three clauses are literally the signature scheme: identity co-sign on genesis, device consent-sig on its own records, sponsor member-sig on adds.
Theft must break two ownerships at once (the device won't consent to the stealing, the owner of record can't be displaced); the accepted price is that a device dropped in the ocean stays on the ledger forever — attached as testimony, detached from every secret.

## Why v1 retires

v1's word check was cryptographically sound: the typed 23 words are the full pairing pubkey, matched exactly by machine against the posted request, whose signature only the pairing private key could produce.
What was wrong sat around it:

- The relay pair slot was ONE clobberable cell per handle_proof, writable by anyone holding the handle (`handle_pair_put` verified nothing) — the genuine request could be evicted remotely, silently, forever. DoS, never capture, but a standing blast surface.
- The 23-word typing burden was a consequence of WAN delivery: with an open slot, the human channel had to carry all 256 bits to pin one key.
- The worker's device→owner index claimed on mere chain listing — anyone who learned a virgin device pubkey could Add it to their own chain first and brick its enrolment (no re-roll: device keys are deterministic from the fingerprint).
- The added device never signed its own membership: conscription was structurally possible, which is the only reason the squat mattered.

## The ceremony (words transport)

1. New device: user types the handle → `identity_seed` + `handle_proof` derived locally. The device is now pre-aimed at exactly one fleet.
2. New device posts a **binding request** to FGTW: `{hp, device_pubkey, t}` signed by its device key AND co-signed by `Ed25519(identity_seed)`. Re-posted at ~3.5 min while the screen is up; withdrawn by the device on green or exit; lapses at the worker's 5-minute freshness otherwise.
3. New device shows its device pubkey as 23 voca words, **masked**: `words = pair_words(device_pubkey XOR blake3::derive_key("photon pair words v1", identity_seed))`. Fixed width 23 (the ~11 spare bits stay spare for versioning; no checksum — the live matcher subsumes it). RED lamp.
4. Old device: the add-device screen pulls the pending request set (member-gated read), kept live by the hub `request` event; for each candidate it locally computes the expected word string and the keyed device name.
5. Human types the words into the old device. Every keystroke prefix-matches against the candidate strings: divergence flags the exact word it happens at; a unique prefix can already show WHICH device is matching, by name.
6. Full 23-word match → both request signatures re-verify → the old device signs the Add with the request's device signature embedded as the **consent egg** → publish. Chain extension is public WAN traffic, as always.
   **Single-phase on the words transport (revised 2026-07-14 after on-device testing):** the typed 256-bit match IS the human confirmation — you can only type the exact words shown on the one device in your hand, so there is no wrong candidate for a green-confirm to guard against. The old device therefore auto-rotates the fleet key immediately on match and returns to the Fleet page; the new device gets its key seconds later. The two-phase human gate (bind → "It's in — finish" → rotate) stays wired but dormant, reserved for the BLE transport, where the radio delivers candidates you did NOT type and the far-screen-green check earns its keep.
7. New device's join loop (unchanged from phase 1): folds the chain with genesis re-verified on EVERY fetch → finds its own pubkey → GREEN — it leaves the words screen and attests IMMEDIATELY, without waiting for the fleet key. Green is unforgeable: the seed never leaves the device, a fake chain fails genesis, the real chain requires sponsor + consent signatures. (First build waited here for the key, which deadlocked the ceremony: the sponsor's human waits for this screen to change before releasing the key.)
8. Human sees green, confirms on the old device → fleet key rotation sealed to the member set including the newcomer → the worker broadcasts the rotation (`fleet` hub event) → the new device's key sync recovers it seconds later. The old device returns to the Fleet page it launched from.
   **The gate is real only because the joiner NEVER rotates itself in**: any member may rotate, so `recover_or_establish_fleet_key` establishes ONLY when no fan-out exists at all (the genesis founder); an existing fan-out without our wrap means "freshly bound, wrap arrives with the sponsor's confirm — wait". Without that rule a just-bound device could mint itself the key and void the two-phase confirm (first build's bug, caught in the first on-device run).
9. Anything other than green → don't confirm: the bind is a keyless ledger entry (testimony of a mistake, powerless without provision). Rotate to the intended set and tombstone locally. There is no unbind — see chain changes.

No time-based UI anywhere: states are event-shown, interaction-cleared; the user standing at the screen is the timeout.

## Lamp states on the new device

- RED waiting: not in the fold yet — normal before the bind lands.
- RED wrong-fleet: chain exists but genesis does not match my seed — the handle is occupied by a foreign genesis; scream.
- GREY: chain unreachable — a network condition, never a verdict.

## The binding request registry

- **Keyed set** per `(hp, device_pubkey)` — a rival request sits beside the genuine one; nothing evicts anything.
- **Write gate**: both signatures verify at the worker; the identity co-sig is checked against the stored chain's genesis `identity_pubkey`, so only the handle's owner can enter the set at all. No chain → no registry (a first device is the genesis path and needs no request).
- **Read gate**: member devices only — the same fold gate `inbox_drain` uses. Nobody else can enumerate pending requests.
- **Lifecycle**: the author withdraws it, or the stamp lapses. The worker NEVER consumes a request — third-party deletion does not exist in this system.
- **Hub**: `pair_evt` gains kind `request` (posted/withdrawn) so the matcher screen updates live; `fleet` stays (drives the green lamp); `matched` retires.

The registry authenticates candidates; it never selects. Selection is the typed words, end to end — a stuffed request that doesn't match the entry is invisible noise.

## Chain changes

- **Add carries consent**: `consent_t` (the request's `t`) + `consent_sig` (the request's device signature), bound into `signing_bytes` exactly like the genesis identity binding (the sponsor commits to the consent it saw; a stray consent on other kinds is rejected like `StrayIdentityBinding`). Fold verifies `consent_sig` under `device_pubkey` and rejects `|eagle_time − consent_t| > 1h` — bilateral forever, ancient-consent replay dead. Conscription is structurally impossible: only a consenting key can be bound, so typo-garbage and imposter keys can't enter the chain at all.
- **Remove is self-signed departure ONLY** (decided 2026-07-13, ahead of the withholding layer, eyes open): fold rejects `signer != device`. Nobody can be expelled; eviction is withholding — today that means rotating the fleet key to a subset; the full layer (S/friendship re-key around a lost device, routing locks) lands with the device-trust bundle. Interim: the fleet page's remove-other UI retires; a lost TEST device is pruned by re-genesis.
- **Genesis unchanged**: self-signed + identity co-sign — both ownerships were already present in it.
- **Flag-day**: consent-less Adds don't fold, no version gate. Existing test fleets (~5 devices, no userbase) wipe and re-genesis.

## Worker changes

- `bindreq_put` / `bindreq_list` / `bindreq_withdraw` replace `pair_put`/`pair_get`/`pack_put`/`pack_get`.
- `fleet_put` verifies consent eggs on Add and self-signature on Remove — free, since worker and clients share `fgtw::fleet::fold`.
- The device→owner index claims ONLY from consent-carrying ops (genesis self-sig, Add consent egg) and releases on self-departure or supersession — never from mere listing. The squat dies.

## Threat ledger

- Slot stuffing: dead — keyed set, nothing evicts the genuine request.
- Registry spam: requires the handle (identity co-sig write gate); invisible anyway (member-gated read, and the words select — noise can't match).
- Shoulder-surfing the words: inert. The pubkey was headed for the public chain anyway; without a request signed by THAT device's key nothing can be bound, and ownership claims need consent evidence. The mask additionally makes the words meaningless outside this fleet.
- Typing words read off an attacker's screen: the only wrong-bind class left, and it requires the human to transcribe from a device that isn't theirs; the bind it produces is keyless until green-confirm, which the victim's own device (red) refuses to justify.
- Typo: diverges from every candidate at the word it happens; a wrong entry never decodes to anything bindable.
- Two fleets pairing in one room: masks differ, so each fleet's words are noise to the other; requests are per-hp besides.
- Ancient-consent replay after a departure: the 1-hour fold window kills it.
- Malicious relay: genesis re-verified on every fetch (kept from the earlier draft); fake green impossible; withholding registry or chain is DoS — loud, never capture.
- Wrong bind among your OWN consenting devices: harmless (it's yours) or unconfirmed (keyless entry, rotate around it, local tombstone).

## Transports — one machine, three selectors

The ceremony is a **candidate-set + selector** machine: the registry authenticates candidates, a selector picks exactly one, everything after selection is identical (bind → green → confirm → rotate).

- **Words** (universal, THIS build): the selector is the full masked word string — it carries its own delivery, needs eyes and fingers, works on every platform with a keyboard.
- **NFC** (phone↔phone, later): the tap delivers the candidate pubkey; physical touch IS the selector; wordless.
- **BLE** (MVP wired 2026-07-14): announce beacons deliver candidates room-wide.
  **PROXIMITY POPULATES THE TAP LIST, NEVER THE REGISTRY.** The old device's AddDevice screen shows a tappable row ONLY for a device it is currently HEARING over the BLE announce beacon (later: an NFC tap). The WAN binding-request registry is *sync only* — it carries the consent signature a tap binds with, but it must NOT populate the selectable list, because a remote attacker who holds the handle can FLOOD the (identity-gated) registry with binding requests; listing registry entries as tap targets would fill the user's finger-reach with decoys. Proximity is the one thing a remote attacker can't fake — they can't broadcast BLE / present an NFC tag in your room. So: heard-nearby → tappable; not-heard → doesn't appear, you type its words (reading them off the physical screen IS the proximity check).
  A tap binds using that device's registry request (consent). Because a name-pick isn't a typed-key match, the tap/proximity path takes the TWO-PHASE gate: bind (keyless) → new device folds + goes green → old device asks "did it turn green?" → confirm → rotate. Wrong pick → your device stays red → don't confirm → keyless entry, no key leaked. The lock word + `word_mac` proof (single-valid-proof abort) remains an OPTIONAL hardening on top of proximity, not required for the MVP. The milestone-A shadow beacon is the radio path (Android advertises + scans; Linux scans via bluer; desktop advertise still pending → a Linux new device isn't heard, so it's added by words).

## What retires (flag-day)

- Worker: pair/pack slots, `PAIR_FRESH_OSC`, hub `matched` kind.
- `fgtw::pair`: `new_pairing_id`, the pairing-key words path (`pair_words` re-targets to the masked device pubkey), `pair_request_signing_bytes`/`pair_matched_signing_bytes`, `PairRequest`.
- Photon: the `post_pairing_request`/`fetch_pairing_request`/`post_pair_matched`/`poll_pair_matched` wrappers; the words-display-of-pairing-key screen and the complete-entry-then-check flow; `spawn_unbind_device` and the fleet-page remove-other arm (returns as self-departure + ostracize-rotation with the device-trust bundle).
- All v1 chains and test fleets (re-genesis).

## Build order

1. `fgtw` crate: masked codec, binding-request signing bytes, consent egg + fold rules, client `bindreq_*` fns. Unit tests: codec round-trip, conscription rejection, replay window, self-departure-only fold.
2. Worker: registry + gates, ownership-index change, hub `request` kind, retire the slots. Deploy.
3. Photon: live matcher screen (old device), join-flow swap + withdraw-on-green (new device), two-phase rotate behind green confirm, retire v1 screens.
4. Live round-trip test (successor to `live_device_add_round_trip`), then the real thing: two devices, type the words, watch the lamp.
