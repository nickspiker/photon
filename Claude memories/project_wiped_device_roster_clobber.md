---
name: project-wiped-device-roster-clobber
description: 2026-08-16 mac clear+re-attest came up contactless - stale oracle fleet key + aead breaker pushed near-empty roster over the slot; guard SHIPPED b83834f, epoch heal verified live
metadata:
  type: project
---

**INCIDENT 2026-08-16:** Nick cleared + re-attested the MacBook (fe46a74b) → no contacts. Root cause chain:
1. Deterministic device identity + surviving fleet/v1 chain = re-attest WITHOUT pairing ([[project-keyring-design]] ops trap) → nothing hands the device the current fleet key.
2. It recovers a SUPERSEDED key from its oracle slot → every fstate/roster pull dies with aead::Error.
3. The aead-exhausted breaker (protocol.rs) assumed the SLOT was stale and re-sealed from local state — but the wiped device is the stale party holding only the attest-minted self row → it clobbered the fleet's roster slot with near-emptiness under a key no sibling holds.

**GUARD SHIPPED @ b83834f:** breaker fires only with local FRIEND rows (non-sibling, non-self — the self row exists on every fresh attest and proves nothing); a friendless device holds for the key instead. The designed heal was verified live via pid-probe: wiped mac eggs with a sibling → mints the next fan-out epoch → wraps egged siblings → siblings adopt (worker monotonic epoch guard) → their pushes become readable → refold-edge pull succeeds. pid-probe now prints the fan-out envelope (epoch + rotator + wraps — plaintext structure) as ground truth.

**Residue:**
- Phone (1be949c1) held no epoch-39 wrap (not re-egged) → its OLD binary's ungated breaker can re-clobber with a stale-key full roster until it re-clutches; converges after re-egg + rotation. Old binaries carry the ungated breaker until next publish.
- FSTATE CHURN (mischaracterized at first as a steady 1/sec drone — it is NOT): full-log timeline showed ALL fstate traffic in the first 21s of launch (19 pushes), then silence. Audit: every roster_updated bump is change-gated on roster-carried fields (name/pin/owner/woven) or the deliberate owner-keepalive; the comparison side is sound. Real cause = spawn_roster_push had NO in-flight guard: every launch edge (re-push, weave claims, keepalive stamps, pong adoptions, reconciles) spawned a CONCURRENT pull-merge-seal-put racer, each fstate event re-pulling every sibling. FIX SHIPPED: coalescing — one push in flight, mid-flight requests queue exactly one follow-up that re-snapshots on the completion edge (roster_push_rx drained in tick, no timers).
- Desktop log dark since 17:22 UTC: hard-logs window expired → SOFT mode batches to RAM, disk flushes only on edges (panic/background/submit/threshold/arming). Not a hang. To see a live desktop's recent lines: arm hard logs or submit.
