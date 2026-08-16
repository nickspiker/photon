---
name: project-colour-pipeline
description: "colour output doctrine — assume wide-gamut panels, tag surface BT.2020 γ2 and ship raw VSF, convert only as fallback; sqrt γ2 not sRGB OETF"
metadata: 
  node_type: memory
  type: project
  originSessionId: a520cdd8-005f-4740-b889-3392012d4947
---

USER DOCTRINE 2026-07-23 (colour output, all platforms):
- Assume the panel is WIDE GAMUT (most monitors are now). An untagged surface is probably on a wonked default anyway, so sRGB-pessimism is wrong.
- Tag the surface as BT.2020, ideally γ=2, ship raw VSF-ish values into it — DON'T convert down to sRGB. If the platform can't do γ2, tag "as close as possible" and keep γ2 pixels.
- A user who truly wants calibration will supply an ICC; only THEN override the assumed space.
- Transfer = **sqrt (γ2)**, NEVER the piecewise sRGB OETF — "sqrt is WAY faster." This is the ferros-native γ2.0 doctrine ([[project_theme_rec2020]], colour_convert.rs already ships γ2.0 into Android's γ2.2 slot as the accepted trade-off).
- Background NOISE never takes a per-pixel conversion (too slow) — roughly convert the base/mask/speckle CONSTANTS to the output space once instead (fluor theme.rs BG_BASE/BG_MASK/BG_SPECKLE are VSF-authored, currently unconverted — the real remaining gap).

STATE 2026-07-23:
- Mac: SOLVED — renderer_wgpu.rs tags the CAMetalLayer with vsf_rgb.icc (full VSF-RGB ICC via setColorspace:), so the surface wants RAW VSF, no conversion. renderer_macos_softbuffer.rs is DEAD CODE (Mac routes thru wgpu). Cleanup = the theme.rs `to_display` non-android branch currently sRGB-CONVERTS, which is WRONG for a VSF-tagged Mac surface — Mac should ship raw.
- Android: surface tagged BT.2020+γ2.2; today converts VSF→2020 via vsf_rgb_to_bt2020. Per doctrine, check if Android can tag VSF-ICC like Mac and ship raw (unify), else keep the rough matrix.
- Windows: IGNORE for now (UpdateLayeredWindow, no tagging).
- Linux: OPEN — Wayland color-management-v1 / X11 _ICC_PROFILE polling; if taggable ship raw, else tag BT.2020 assumption.
- softbuffer: user has a FORK (github.com/nickspiker/softbuffer, used on Android per Cargo). "softbuffer needs to go" really = add colourspace tagging where softbuffer blocks it (Linux/Windows); Mac already bypassed it via wgpu.
- theme.rs pass IN PROGRESS: 30 LazyLock statics via `c()` = to_display + dark(fmt()); ring tiers added (RING_LAN cyan / ONLINE green / RELAY 0xFFB000 amber / OFFLINE grey) + ring_tier_colour() picks LAN by private/link-local/ULA validated_path; BG_BASE_WARNING takes fmt not dark (noise-math). The `to_display` platform branch is what needs the doctrine applied.

Related: [[project_theme_rec2020]], [[project_android_color_pipeline_floor]], [[project_presence_vs_online]].
