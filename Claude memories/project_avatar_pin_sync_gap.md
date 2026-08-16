---
name: project-avatar-pin-sync-gap
description: "fleet avatar pin never stabilizes in global settings — multiple devices each mint a fresh pin (LWW clobber), so cross-device avatar/name sync fails; NOT android-specific"
metadata: 
  node_type: memory
  type: project
  originSessionId: a520cdd8-005f-4740-b889-3392012d4947
---

OBSERVED 2026-07-23 (Nick 3-device fleet, 0.43.0): user reports "Android avatars/names don't sync from FGTW." Log truth: NOT android-specific and avatars DO partially work (peer avatars for friends 633219a1/8d2d1b2b/7ff3835f install fine). The real gap is the FLEET SELF avatar pin.
Symptom in logs: fleet slot persistently shows `state pulled — 5 roster, u3(1) global setting(s), u3(0) device map(s)`. The profile (profile.name, profile.avatar_pin, profile.avatar_ts) is NOT in global settings — only 1 global key present (likely updates.auto). So:
- BOTH desktop 90e571bf AND new device fe46a74b log `AVATAR: generated a fresh random pin (fleet-synced)` seconds apart — each mints its OWN pin because the pull found none to adopt.
- Different pins per device → a device can't decrypt a sibling's uploaded avatar → "doesn't sync." ensure_avatar_pin (photon_app.rs ~7617) is supposed to probe-then-generate ONCE fleet-wide; instead it regenerates per device.
Suspected cause: ensure_avatar_pin generates + settings_set("profile.avatar_pin") but the pin isn't landing/surviving in the pushed global settings (device map(s)=0 too — the per-device layer isn't reaching the slot either). Either the profile keys aren't `linked` (so they go to a device map that isn't pushed), or a race: generate fires before the first fleet-state pull completes, and then LWW between two fresh pins never converges. spawn_settings_push pushes global+devices; slot shows both nearly empty → the push either isn't carrying profile.* or is being overwritten.
NOT yet root-caused to a line; NOT fixed. Next: confirm whether profile.* is linked (→global) vs device-scoped, and whether ensure_avatar_pin runs BEFORE the initial fleet-state pull (the probe-then-generate ordering bug — generate must wait for the pull or it clobbers).

FIXED 2026-07-23 (b5fe9c6): probe-then-generate — `fleet_state_pulled` flag gates ensure_avatar_pin's MINT path (lookup always allowed); first pull re-calls publish_avatar_pin to adopt-or-mint-safely. Pending live verification.
The repetitive avatar RE-DOWNLOAD the user saw is the SAME root cause: spawn_avatar_download is cache-first + deduped per-session on `hp`, but the vault cache keys on (party_id, avatar_pin) — so every pin change missed the cache and re-fetched from FGTW under the new pin. Pin churn = repeated re-download. Stabilizing the pin fixes both. NOT a separate avatar bug.

Related: [[project-avatar-bearer-pin-gap]] (the removal-rotate pin work), [[project-fleet-braid-plane]], [[project-storage-layering]], [[project-update-flow]] (the update-redownload dedup, same session).
