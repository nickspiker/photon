---
name: project-colour-pipeline
description: "colour output doctrine — assume wide-gamut panels, tag the surface (VSF ICC on macOS, BT.2020 γ2 elsewhere), sqrt γ2 never the sRGB OETF, theme constants converted once at compile time; Linux/Windows tagging open"
metadata:
  type: project
---

Nick's doctrine (2026-07-23, all platforms):
- Assume the panel is WIDE GAMUT; sRGB pessimism is wrong. Tag the surface BT.2020 (ideally γ2) and ship VSF-ish values; never convert down to sRGB. Only a user-supplied ICC overrides the assumed space.
- Transfer = sqrt (γ2), NEVER the piecewise sRGB OETF ("sqrt is WAY faster").
- Background noise never takes a per-pixel conversion: convert the constants once.

**State (checked 2026-10-17):**
- macOS: renderer_wgpu tags the CAMetalLayer with vsf_rgb.icc, so the buffer IS VSF RGB (fluor `theme::vsf` = identity there).
- Elsewhere: fluor `theme::vsf` bakes VSF RGB → Rec.2020 γ2 at compile time (Q16, const integer sqrt); photon's `to_display` runs the same math at runtime; Android's surface is BT.2020-tagged.
- Linux (Wayland color-management / X11 _ICC_PROFILE) and Windows (UpdateLayeredWindow, no tagging): untagged, OPEN.
- vsf::colour matrix constants are COLUMN-major (see [[project_beams]]).

Related: [[project_theme_rec2020]], [[project_android_color_pipeline_floor]].
