---
name: project-settings-typed-values
description: fstate v7 SHIPPED 2026-08-16 - every settings value is natively typed VSF (fgtw df3eefa + photon 620734a); v6 compat window; window geometry persisted as four typed keys
metadata:
  type: project
---

**SHIPPED 2026-08-16 (fgtw df3eefa + photon 620734a + fluor 0f30236):** settings values are natively typed VSF end to end — the v(b'r') raw-blob wrapper retired (Nick: "proper VSF all the way... it NEEDS to be done for EVERY KEY").

Type map: display.zoom=f5 · display.window.{x,y}=i5 .{w,h}=u5 (DEVICE-LOCAL, save on settle edge + close flush, one-shot restore with off-monitor guard) · updates.auto/share.*/unlock-tombstone=u0 · logs.hard/profile.avatar_ts=e · profile.*/theme/_custom=x · fleet.locked.*/released.*=ke · avatar_pin/react.recent=hR. ONE deliberate raw survivor: the legacy fleet.locked one-blob (concatenated pubkeys, read forever).

Compat: FSTATE_VERSION 6→7 with v6 in the read window — old values arrive AS the v'r' wrapper; per-type getters (as_f32/as_bool/as_osc/as_text/as_key32/as_bytes in photon fleet_settings.rs) each carry that one fallback; every write re-types. Old builds reject v7 docs (loud, no silent entry drops — a v6 device keeps its lock cache; convergence as devices update). Merge tiebreak = flattened wire bytes; Eq dropped from value-bearing structs (floats).

**Why (doctrine):** anonymous packed scalars are the ten-year trap — vsfinfo-illegible, endianness/field-order folklore. Same disease class as the worker registers (fixed 2026-08-15) and kete's raw ints.

**Watch on field rollout:** first v7 push from an updated device; siblings on v6 stall settings-sync (loudly) until updated — the approved soft flag day.
