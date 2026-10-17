---
name: feedback-legacy-first
description: "Porting legacy compositing ops to fluor: faithful visible-RGB RMW first; doctrine-clean fluor under-blend chain is a later cleanup"
metadata:
  type: feedback
---

When porting visual code from Photon's legacy compositing to the fluor path, default to **legacy-identical visible-RGB RMW** (read pixel → XOR to visible → legacy op → XOR back, preserve α). Don't reshape ops into fluor's topmost-first `under()` chain unless the op is expressible that way (a plain fill is; `sqrt(c*scale + bg²)` reads the bg, which under-blend forbids via the opaque-top early-out).

**Why:** an under-blend rewrite of `chromatic_wave` + `photon_logo` produced subtly wrong visuals (wave brightness curve off); Nick walked it back. The legacy ops are the source of truth for the look.

**How to apply:**
- Use the ops legacy used (wrap-add, sqrt-quadrature blend, alpha-weighted darken), translated to fluor's α + darkness storage by XORing visible at read and write.
- Compose bg-first / topmost-last (noise → wave → logo) when any op reads the bg.
- Text needs the u8-scratch + manual visible-RGB blend; `draw_text_center_u32` under-blends and early-outs over opaque pixels.
- Migration to the `under()` chain is a separate later cleanup, never part of the port.
