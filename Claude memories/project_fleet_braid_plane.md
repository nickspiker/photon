---
name: project-fleet-braid-plane
description: Fleet-shared friendships must ride braid.md §14 fleet plane; single-egg fstate-snapshot plan REJECTED 2026-07-23; build order step 1 = removal rotates
metadata: 
  node_type: memory
  type: project
  originSessionId: f4272721-c713-4a82-a97a-db8106029756
---

2026-07-23: the "fleet-shared friendships via fstate slot" plan (plans/mellow-painting-diffie.md, now deleted) was REJECTED and must not resurface.
It sealed the friendship's live chain pads as a single AEAD blob under the standing fleet key and LWW-merged it — the "one egg" problem.
§14 of braid.md (fully spec'd, red-teamed, ⏳ unbuilt) forbids it four ways: standing fleet key alone must not open pre-horizon content (forward secrecy = reservoir burn + per-checkpoint keys crypto-shredded on-device); epoch advances per sealed checkpoint never per message (per-message re-seal forks the reservoir under concurrent senders); message substrate is grow-only content-addressed union-merge, never LWW-on-blob (LWW is the roster's model only); nonce discipline is per-message tag T = blake3(device_private ‖ eagle_time) folded into the key seed, not a raw AEAD nonce on a standing key.

**Decision:** build §14 properly, in its §14.12 order. Step 1 = "Removal rotates" — per-member fan-out key rotation (§14.2). This is foundation AND closes a live security gap: `unbind_device` never rotates today, so a removed device keeps reading everything.
Steps: fan-out rotation → recovery slot → union-merge sync channel → checkpoint spine + reservoir burn → horizon/crypto-shred → linearizer. Crypto review pass still owed per §14 itself.

Context that motivated it (still true, still wanted): a new device currently runs its OWN CLUTCH against a friend the fleet already holds (violates fleet-sync.md §4.2 "one CLUTCH, the fleet inherits"); a year-later device must land at the CURRENT rolling chain position (birth eggs useless — forward secrecy burned old keys); Chain = self-contained 512-link 16KB buffer per participant, current key = links[511], `save_friendship_chains` serializes exactly that (~32KB/friendship).

STEP 1 SHIPPED 2026-07-23 (photon @ a5c12e4, fgtw @ 1c79c0a, both pushed): `fanout_needs_rotation(wraps, members)` = wraps > members (fgtw::fanout, unit-tested; strictly-greater so the two-phase ADD window never auto-rotates), sentinel lives inside photon's `spawn_fleet_key_sync` (runs on attest + every `fleet` bump + a reconcile shrink), heal = pull fstate under OLD cached key → rotate to survivors (verified members) → cache → push CRDT merge under NEW key → winner-only avatar-pin rotate via `fleet_rotated_tx` drain → `rotate_avatar_pin` (no new image, [[project-avatar-bearer-pin-gap]]).
`fleet_heal_busy` latch parks concurrent syncs (stale-cache window); worker's monotonic epoch guard converges racing siblings, stale loser adopts winner's key.
Live E2E `live_removal_heal_round_trip` PASSES against fgtw.org: sentinel fires on depart, settings survive the re-seal, leaver locked out of fan-out AND slot, sentinel quiet after.
History pages = verified NO-OP (never at rest under fleet key — sealed per-frame at send/serve time, self-heal via sweep); fstate is the only at-rest surface.
braid.md updated: §14.2 shipped note, §14.11 G7 equal-counts residue (simultaneous depart+bind invisible to the count check, self-healing), §14.12 items 1-2 marked live.
Next per §14.12: item 3 = union-merge per-conversation sync channel (§14.5) with anti-entropy.

§4.2 CEREMONY-OWNER CLAIM BUILT 2026-07-23 (the "who runs the one CLUTCH" front-half; chain-travel inheritance stays §14): roster entry grows ceremony_owner[32]+woven (ROSTER_TAG bump PRST1→PRST2 = flag-day, roster re-syncs — MIXED-VERSION fleets split rosters until every device updates!). Adding device claims at add; keygen queue PARKS friend contacts claimed by a present sibling (presence-driven takeover when owner absent; claim-on-pickup for unclaimed/legacy; LWW settles races); seal pushes woven=true; parked UI = "weaving on <device>…" / "secured on <device> — replies visible here; send from there (for now)" (compose stays chain-gated). Interim semantics: read-everywhere (fleet history sync), send-from-the-woven-device.

Related: [[project-keyring-design]], [[project-fleet-routing-scale]], [[project-history-recovery]], [[project-device-loaners]] (removal = consent-only; eviction lives at the re-key layer — §14 step 1 is that layer for the fleet plane).

**UPDATE 2026-08-15 (from braid.md §14.12, which is kept current):** step 1 removal-rotates SHIPPED 2026-07-23 (shrink sentinel, fstate preserved, avatar bearer pin rotated by the winner) — the plans/ file is gone, the doc is the tracker. Remaining ladder: 3 union-merge sync channel (§14.5) → 4 checkpoint spine + reservoir burn (§14.3-14.4) → 5 horizon + crypto-shred (§14.7-14.8) → 6 linearizer (§14.6, last).
