---
name: project_ceremony_claim_first
description: BUILT 2026-10-01 — ceremony ownership is CLAIM-FIRST (the adding device's roster claim stands while it is live; only a dead claim falls to the lowest live device, which takes over by writing a fresh claim); OPEN restart re-keys offline friends
metadata:
  type: project
---

Nick 2026-10-01: "pin that to the first device to grab the ball, rather than a low hash or something for a device that may not be online very often."
Before, era.rs `recompute_ceremony_owners` pointed every friendship at one computed owner (lowest live device), so a pocketed phone could own every ceremony.

**Now (era.rs):** `device_live(d)` = ours, or a fold member not locked / locked_out / probed-offline. Per friendship the `ceremony_owner` CLAIM (set at add in launch.rs, LWW-synced in the roster, persisted) stands while `device_live`; else the `era_owner` fallback (lowest live). A fallback that is THIS device takes over: writes the claim, bumps roster_updated, persists, `spawn_roster_push()` — the woken claimant adopts it.
**Why it is also a consent fix:** `consent_mutual` defaults TRUE in Contact::new and is not a roster field, so a sibling adopting a fresh friend row could keygen+offer at once; claim-first parks it behind the adding device.
Also: "waiting for them to add you" status (`Msg::ClutchWaitingTheirAdd`) replaces a lying "0/12 making keys"; the stream filter pill hides on the barren CLUTCH screen.

**OPEN:** every restart re-keys ~7 offline Pending friends (offers that "reached NOBODY"), and one contact got two keygens per start (double pickup). Field verify pending. Related: [[project_consent_order_deadlock]].
