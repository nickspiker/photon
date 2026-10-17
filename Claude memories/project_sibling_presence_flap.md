---
name: project-sibling-presence-flap
description: "CLOSED 2026-09-07: relay-only siblings flapped online/offline + parked at 11/12 because relayed pongs arrived after the match window; two-tier pending window fixed it"
metadata:
  type: project
---

Field 2026-09-07 (MacBook fe46a74b on LAN ↔ Pixel cacbc223 on cellular IPv6): no direct path, everything on the relay.
Both sides logged `unmatched pong … (late/twin/announce; no addr adoption)` every cycle → timeout strikes → offline, then a sync record flipped it online → opposite-phase flap; the sibling ceremony parked at 11/12 waiting for a round trip that never validated.

**Fix (status.rs):** two-tier pending window — strike at `PING_STRIKE_AFTER` (5 s) but stay nonce-matchable until `PING_MATCH_WINDOW` (90 s).
A late match honors presence (purge, strike reset, sync tail) while address-flavored actions (adoption, PT retarget, reflexive echo) stay gated to fresh matches.

**How to apply:** a pong arriving late over the relay is still proof of life; never let a timing window decide presence for relay-only peers.
Ghost 1be949c1 roster purge: see [[project_clutch_completion_rebroadcast]].
