---
name: project-settings-typed-values
description: "fstate v7 SHIPPED 2026-08-16: every settings value is natively typed VSF (no raw-blob wrapper); window geometry = typed device-local keys"
metadata:
  type: project
---

SHIPPED 2026-08-16 (fgtw df3eefa + photon 620734a + fluor 0f30236): settings values are natively typed VSF end to end; the v'r' raw-blob wrapper is retired (Nick: "proper VSF all the way... for EVERY KEY").

Type map: display.zoom=f5 · display.window.{x,y}=i5, {w,h}=u5 (device-local) · updates.auto/share.*/unlock-tombstone=u0 · logs.hard/profile.avatar_ts=e · profile.*/theme/_custom=x · fleet.locked.*/released.*=ke · avatar_pin/react.recent=hR.
Getters live in photon fleet_settings.rs (as_f32/as_bool/as_osc/as_text/as_key32/as_bytes). Merge tiebreak = flattened wire bytes.

**Why:** anonymous packed scalars are the ten-year trap — illegible to vsfinfo, endianness/field-order folklore.
**How to apply:** a new setting gets a real VSF type, never a packed blob. See [[feedback_numbers_binary_at_rest]].
