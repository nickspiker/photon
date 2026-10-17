---
name: project-theme-rec2020
description: "theme colours are authored in VSF RGB and routed thru fluor's const fn vsf() to Rec.2020 output on every platform; never a raw framebuffer constant"
metadata:
  type: project
---

Nick 2026-07-23: theme colours were "lazily passed thru" — authored in VSF RGB but written to the framebuffer unconverted.
Done 2026-09-12: fluor's whole palette routes thru `const fn vsf()` in fluor/src/theme.rs (identity on macOS's ICC-tagged surface, a baked Q16 matrix elsewhere); photon's raw backdrops became palette entries (VIEWER_BG/CONSENT_BG). The avatar pipeline's `vsf_rgb_to_bt2020` is the same conversion.

**How to apply:** any new colour is a palette entry thru `vsf()`; Rec.2020 is the output target on ALL platforms.
Related: [[project_android_color_pipeline_floor]], [[project_presence_vs_online]] (ring tier colours).
