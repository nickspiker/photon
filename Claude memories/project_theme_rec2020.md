---
name: project-theme-rec2020
description: theme.rs colours (fluor + photon) are VSF RGB lazily passed thru — must convert to Rec.2020 and target Rec.2020 output on ALL platforms
metadata: 
  node_type: memory
  type: project
  originSessionId: f4272721-c713-4a82-a97a-db8106029756
---

User 2026-07-23: the theme colours in fluor's and photon's theme.rs were "lazily passed thru" — the constants are authored in VSF RGB but land in the framebuffer unconverted.
Fix = run them thru the VSF-RGB→Rec.2020 conversion (the avatar pipeline's `vsf_rgb_to_bt2020` is the existing correct path) and make Rec.2020 the colour output target on EVERY platform, not just the Android BT.2020-tagged buffer ([[project-android-color-pipeline-floor]] already established that floor on Android).

Related: [[project-presence-vs-online]] (ring tier colours are specified in VSF RGB and ride this same conversion), [[project-party-colour-perceptual]].
