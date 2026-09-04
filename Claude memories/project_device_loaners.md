---
name: project_device_loaners
description: "Loaner devices DECIDED 2026-09-04: LEASE model (docs/device-lease.md) — grant/recall on the brand, title never moves; airport delegated guest session = stage 2 with pairing-v2 + fleet-inbox cluster"
metadata: 
  node_type: memory
  type: project
  originSessionId: 9dd5621d-5f68-4b43-83bd-807531b898e2
---

Loaner-device design DISCUSSED 2026-07-12 (user-initiated), not yet spec'd into docs — open questions pending user.

Facts established:
- One-owner-per-device guard is node-side LIVE and covers genesis too (whole posted chain is checked) — "first attest needs owned devices" already holds.
- `device_owned` is a specific worker error frame and the client plumbing DOES surface it on the owner's add screen — but only post-words (bind is the first server contact) and only owner-side (the joiner is never told). Client-side up-front probe + joiner-side verdict are the queued follow-ups.
- `[]x` shred RELEASES the claim (unbind, blank slate); session-clear/app-data-clear keeps it. Which reset was used decides whether a subsequent add SHOULD decline.

The loaner middle state: claimed-dormant (de-attest keeps claims, handle off the air). HARD CONSTRAINT: de-attest must NOT release claims, else one machine can mint handle after handle (sybil laundering). Dormancy relaxes ONLY bind-into-another-fleet (pairing possession = consent), never genesis.

**DECIDED 2026-09-04 — the LEASE (docs/device-lease.md is canonical)**: option (b) won and grew. Grant = owner-fleet-signed brand annotation naming the guest identity; worker attest rule = brand owner OR live grant; recall = owner-signed edge at the ROUTING layer (the lockout machinery pointed at a guest); guest vault seals under their handle (mutual privacy); owner re-attest on recalled hardware wipes foreign vaults; recall never touches the guest's own fleet membership (they bilaterally depart at leisure). Release-folded-into-Approve REJECTED — with leases, "someone uses my device" is a lease, and title transfer keeps its two-tap friction.
**The airport vision (stage 2)**: borrow a STRANGER's device, approve on your watch, everything appears — the handle NEVER touches borrowed hardware (seed = BLAKE3(handle)); a delegated SESSION key (routing + fleet-streamed history, never the seed) lands instead, killable from any owned device. Builds with the pairing-v2 + fleet-inbox + session-capsule cluster, after voice calls. Stage 1 (household lease, guest types own handle) can ship alone.
Also flagged 2026-09-04: a WORDING/FLOW sweep across most pages is wanted — the lease/recall/release/retire vocabulary should land with it.
**Generalized same day into docs/key-custody.md (the voucher ladder)**: wairua (same boot) → FLEET vouches (approve on an owned device, fleet delivers session material + wrapped vault root — seed at rest NOWHERE, never on a wire) → handle typed only as root of last resort. Guest session and owned-device wake become ONE approval flow; lockout = fleet refusing to vouch. Wrapped-root design deliberately deferred to the fleet-key redesign's ira-wrap (don't front-run Nick's review).

Superseded original question:
1. Loan ownership model: (a) transfer-on-bind (owner index flips; recall needs borrower cooperation) vs (b) loan annotation (owner stays, `loaned_to` added; unilateral recall enforced at announce time — I argued for (b)).
2. Dormancy fleet-wide only, or per-device loanability.
3. Stated (not accidental): the handle stays occupied thru dormancy — forced anyway since seeds derive from the handle string.

Loan-recall announcements ride [[project_fleet_inbox]]. Revocation/eviction = remove+rotate, bundles with [[project_rekey_attack_surface]] device-remove work.

**Removal consent model — REVISED 2026-08-31 (see [[self-only-removal]]): departures are BILATERAL now** — leaver's signed request + a surviving member's countersignature (mirror of add); bare self-departure is refused at the worker (it was the device-laundering vector). The device's own signature remains mandatory (the 2026-07-12 "requires a sig from the device, period" mandate holds); what changed is that it alone no longer suffices. The chain records consent-only facts: bilateral adds + bilateral departures, nothing else. No remote removal op, no tombstone chain op, no contest windows, no custodes in device-removal (custodes = identity recovery only). Kills thief-evicts-owner wholesale (today a stolen member can unbind your devices + rotate you out — must be rewired to this model; `clean_device_for_reuse` self-signout is already the pattern).

Accepted consequences (design constraints, not footnotes):
1. Fleet key = readable by every ever-added device until IDENTITY re-key (rotation no longer evicts; an un-removed member's ihi recovers every epoch, no handle needed) → fleet key must only gate low-blast-radius state (roster); nothing compounding behind it.
2. Hostile-live-device containment lives ABOVE the fleet: session decay (boot-lock), vault handle-gating, S/friendship-layer re-key (friends re-key to owner via social-graph authority — asymmetric, thief can't counter), microwave for hardware in hand.
3. REFINED 2026-07-17 ([[project_identity_never_dies]]): the claim releases ONLY by owner consent — two-signature retire (device self-departs, surviving member signs device_release; SHIPPED). Lost/stolen hardware stays BURNED (nobody left to consent it free... except the owner, deliberately); factory reset still rolls the fingerprint key (deter-in-app only, until PIPE).
4. Tombstone = purely LOCAL UI annotation ("presumed dead" icon on surviving devices), zero protocol weight.
5. Layering rule: revocable OPINIONS (loaner recall, worker announce-refusal, peer de-ranking) live at the routing/trust layer, never as chain ops. A "lock", if ever wanted, returns there.
Also: dev-build seed-logging leak flagged (handle_query.rs Development: identity_seed line lands plaintext in adb-readable photon.log.vsf — breaks "thief doesn't have my handle"; removal offered, pending). Handle should be treated as display-secret in UI (reveal-gated).
