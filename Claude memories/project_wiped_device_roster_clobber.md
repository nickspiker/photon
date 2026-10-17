---
name: project-wiped-device-roster-clobber
description: "CLOSED 2026-08-16: a wiped+re-attested Mac pushed a near-empty roster over the fleet slot under a stale key; breaker now needs local friend rows; roster push coalesced"
metadata:
  type: project
---

2026-08-16: the MacBook (fe46a74b) was cleared and re-attested and came up contactless.
Chain: deterministic device identity + surviving fleet chain = re-attest without pairing → it recovered a SUPERSEDED fleet key from its oracle slot → every fstate pull failed AEAD → the aead-exhausted breaker (protocol.rs) re-sealed from local state and clobbered the fleet roster with near-emptiness.

**Fixed (b83834f):** the breaker fires only when the device holds local FRIEND rows (the self row exists on every fresh attest and proves nothing); a friendless device waits for the key. Heal path verified: wiped device eggs with a sibling → next fan-out epoch → siblings adopt → pulls succeed.
Also fixed: `spawn_roster_push` had no in-flight guard (concurrent pull-merge-seal-put racers at launch) → one push in flight, one queued follow-up re-snapshots on the completion edge.

**How to apply:** a recovery breaker must ask "am I the stale party?" before overwriting shared state.
Related: [[project_keyring_design]], [[re-clutch-never-store]].
