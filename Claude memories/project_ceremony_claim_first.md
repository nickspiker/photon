---
name: project_ceremony_claim_first
description: BUILT 2026-10-01 — ceremony ownership is CLAIM-FIRST: the adding device's roster claim stands while that device is live; only a dead/absent claim falls to the computed lowest-live device, which then TAKES OVER by writing a fresh claim + roster push (the sleepy claimant adopts it on waking); plus the honest "waiting for them to add you" status and the stream filter pill hidden on the barren CLUTCH screen
metadata:
  type: project
---

Nick 2026-10-01: "pin that to the first device to grab the ball, rather than a low hash or something for a device that may not be online very often." Before: era.rs `recompute_ceremony_owners` pointed EVERY friendship at one fleet-wide computed owner (lowest device pubkey among fold members not locked / not probed-offline, unprobed = present) on every evidence edge, overwriting the adding device's claim; a phone that answered one pong but slept in a pocket could own every ceremony until three timeouts.

**Now (era.rs):** `device_live(d)` = ours, or a fold member (sibling list when the fold is unknown) not locked / locked_out / probed-offline. Per friendship: `ceremony_owner` (the CLAIM: set at add in launch.rs, LWW-synced thru the roster entry, persisted in storage/contacts.rs) stands while `device_live`; else owner = `era_owner` fallback (lowest live). When the fallback is THIS device it takes over: writes the claim, bumps roster_updated, persists, `spawn_roster_push()` — so the woken claimant adopts the newer entry instead of reclaiming. Discard-on-park unchanged. Why it is also a consent fix: `consent_mutual` defaults TRUE in Contact::new and is NOT a roster field, so a sibling adopting a fresh friend row believes consent is mutual — under the hash rule it could keygen+offer at once; under claim-first it parks because the adding device (which holds consent=false until their knock/offer) is the owner.

**Also 2026-10-01:** `contact_status_line` returns "waiting for them to add you" (Msg::ClutchWaitingTheirAdd) while a friend row has no consent, no keys, no chain (self excluded via remote_count) — "0/12 making keys" was a lie while the queue waited on consent. The conversation's stream filter pill (All/Waves/Text) draws only when `is_self || chain_woven || owner_woven || rows exist` — never on the barren CLUTCH screen.

**Seen in the desktop's live log the same day, not yet fixed:** every restart re-keys ~7 offline Pending friends (4 restarts × 7 offers that "reached NOBODY"), and contact 63c4ceca got TWO keygens per start (double pickup).

Field verify pending for all of it. Unstick for the already-stuck Jeff pair: Jeff boots + re-adds Nick (his knock then lands on a roster match).
