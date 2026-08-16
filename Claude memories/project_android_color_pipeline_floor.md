---
name: project-android-color-pipeline-floor
description: Empirical floor for 1:1 panel rendering on Android — practical limits established 2026-06-10 on Pixel 8 Pro
metadata: 
  node_type: memory
  type: project
  originSessionId: b7fe9ef3-87b9-47e0-942d-d7683b56b82d
---

Empirical floor for "draw 1:1 to the panel" on stock Android, measured 2026-06-10 on a Pixel 8 Pro (Android 16 / CP1A.260505.005) with photon's swatch bar (R/G/B/C/M/Y/W/K solid rects):

**Working pipeline (~2% red bleed in green/cyan, irreducible without root + vendor-init intervention):**
- Manifest: `android:colorMode="wideColorGamut"`
- Window: `preferMinimalPostProcessing = true`
- ANativeWindow buffer tagged custom dataspace `STANDARD_BT2020 | TRANSFER_GAMMA2_2 | RANGE_FULL = 151388160` via `ANativeWindow_setBuffersDataSpace` (dlsym'd from libandroid.so for API 28+ compatibility)
- BT.2020 because photon's spectral pipeline can synthesize wider than P3
- γ=2.2 (closest named transfer to photon's actual γ=2.0; intentional slight darkness rather than replacing 2-cycle sqrt with 50-cycle powf)

**What does NOT work to reach ColorMode::NATIVE:**
- `Surface.setColorSpaceAgnostic(boolean)` — does not exist as public method on `android.view.Surface`. Lives only on `SurfaceControl.Transaction` as `@SystemApi` (signature-app only).
- `cmd surfaceflinger setActiveColorMode 0` — "Can't find service: surfaceflinger" (blocked from shell even with su).
- `setprop persist.sys.sf.native_mode 1` + reboot — Pixel vendor init clobbers the prop back to 0 at boot.
- Magisk root via `adb shell su` — works for running commands as root but `adb root` itself still fails because `adbd` is production-built. Direct su commands work but the prop reset issue makes this irrelevant.
- Settings → Display → Colors → "Natural" — maps to `ColorMode::SRGB (7)` not `ColorMode::NATIVE (0)`.

**Why:** The residual 2% bleed is the per-device calibration LUT in the display HAL, running after SurfaceFlinger composition. Each Pixel unit gets a factory-calibrated 3D LUT to normalise panel-to-panel variance to a published "Display P3" spec — this hides manufacturing variance from users at the cost of preventing any app (or even any rooted user) from seeing raw panel-native primaries.

**To go lower would require:**
- A Magisk module that re-sets `persist.sys.sf.native_mode` after vendor init runs, then bounces SurfaceFlinger
- Or patching the vendor init scripts directly
- Or direct binder call into SurfaceFlinger's `setActiveColorMode` transaction (binder code varies by Android version)
- Or a custom ROM that exposes ColorMode::NATIVE in Settings (some LineageOS builds do this)

**Implication for ferros:** On stock Android (any vendor, not just Pixel) the panel calibration LUT is non-negotiable from an unprivileged app. If ferros needs true 1:1 panel access on Android, the design assumption must be "users with root + Magisk module unlock it themselves," not "ferros bypasses it from userspace." Where ferros wins this fight is on its own kernel + display driver on bespoke hardware — there the entire pipeline is yours and there's no LUT unless you put one in. On non-rooted Android, ferros on this hardware tops out at exactly where photon does: ~2% calibration LUT residual.
