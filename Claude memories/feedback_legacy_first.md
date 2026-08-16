---
name: feedback-legacy-first
description: "For Photon UI ports from legacy compositing.rs, faithful visible-RGB RMW comes first; doctrine-clean fluor under-blend chain is a Phase 5 concern."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: a9f51934-7668-45d4-b896-93ebab974b35
---

When porting visual code from Photon's legacy `compositing.rs` to the fluor-desktop path, default to **legacy-identical visible-RGB RMW** (read pixel → XOR to visible → legacy op → XOR back, preserve α). Don't re-shape ops into fluor's topmost-first `under()` chain unless the legacy op happens to be expressible that way (a plain opaque/translucent fill is; `sqrt(c*scale + bg²)` is not — it reads the bg, which under-blend forbids via the opaque-top early-out).

**Why:** the user spent a session walking me back from an under-blend rewrite of `chromatic_wave` + `photon_logo` that produced subtly-wrong visuals (wave brightness curve off, "split chromaticity / intensity" framing was nonsense since legacy never separated them). The legacy ops are the source of truth for the visual; faithful porting is what's wanted.

**How to apply:**
- Pick the compose ops the legacy actually used (wrap-add, sqrt-quadrature blend, alpha-weighted darken). Translate them straight to fluor's α + darkness storage by XORing visible at read and write, preserving α.
- Compose order is bg-first / topmost-last (noise → wave → logo) when any op reads the bg.
- Text body needs the u8-scratch + manual visible-RGB blend path; `draw_text_center_u32` under-blends and will early-out over opaque pixels.
- Doctrine-clean migration to fluor's `under()` chain is the Phase 5 cleanup per [[buzzing-puzzling-yao]]'s plan — not a Phase 1 concern.
