---
name: project-android-color-pipeline-floor
description: "Android 1:1 panel floor (Pixel 8 Pro, 2026-06-10): wideColorGamut + minimal post-processing + BT.2020/γ2.2 dataspace leaves ~2% irreducible bleed from the per-unit calibration LUT"
metadata:
  type: reference
---

Measured 2026-06-10 on a Pixel 8 Pro (Android 16) with photon's R/G/B/C/M/Y/W/K swatch bar.

**Working pipeline (~2% red bleed in green/cyan, irreducible unrooted):** manifest `android:colorMode="wideColorGamut"`; window `preferMinimalPostProcessing = true` (PhotonActivity.kt); buffers tagged `STANDARD_BT2020 | TRANSFER_GAMMA2_2 | RANGE_FULL` via dlsym'd `ANativeWindow_setBuffersDataSpace` (now fluor/src/host/android/surface.rs). BT.2020 because the spectral pipeline synthesizes wider than P3; γ2.2 is the closest named transfer to photon's γ2.0.

**Dead ends to ColorMode::NATIVE:** `Surface.setColorSpaceAgnostic` (only @SystemApi on SurfaceControl.Transaction); `cmd surfaceflinger setActiveColorMode` (blocked even with su); `persist.sys.sf.native_mode 1` (vendor init resets it at boot); Settings "Natural" (= SRGB, not NATIVE).

**Why:** the residual is the display HAL's factory-calibrated per-unit 3D LUT, applied after SurfaceFlinger — no unprivileged app can bypass it.
**How to apply:** don't chase the last 2% on stock Android; lower needs root + a Magisk module re-setting native_mode, vendor init patches, or a custom ROM. ferros only wins this on its own kernel/display driver.
