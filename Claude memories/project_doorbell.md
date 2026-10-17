---
name: project_doorbell
description: "Reachability doorbell v1 BUILT 2026-07-19 (docs/reachability-doorbell.md status header is canonical); OPEN: sibling bells overwrite each other (hp-keyed), no opt-in toggle, no TCP tier-1, no UnifiedPush client"
metadata:
  type: project
---

The status header of docs/reachability-doorbell.md is canonical (what shipped, what didn't). In short: bells are `kind:address` strings (`fcm:<project>:<token>`, `up:<url>`) published device-signed under handle_proof via `bell_put`; `ring` makes the worker send an empty high-priority FCM data push (or POST to `up:`); a dozed contact (`Contact::last_heard`, 90 s) with a due retransmit or a parked ceremony triggers it. The bell is a `wake()` for a remote phone; content never rides it.

**OPEN (v1 limits):** bells are hp-keyed, so FLEET SIBLINGS OVERWRITE each other's bell (fix = bells/<hp>/<dev>); Android publishes unconditionally (the off-by-default opt-in row is unbuilt); TCP/WS keepalive tier-1 and the UnifiedPush client are unbuilt.
A wake only helps if the app survives in the background: [[project_android_headless_dark]].
**How to apply:** worker secret `FCM_SERVICE_ACCOUNT` on the `fgtw` worker = the fgtw-90220 service-account JSON; rotation = Firebase console new key + `wrangler secret put`, no code change.
