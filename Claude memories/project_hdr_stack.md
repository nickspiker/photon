---
name: project_hdr_stack
description: HDR stack — OWNED by the photon session since 2026-10-20 (handover from the vsf session); g2t buffer + final table landed, platform HDR surfaces open
metadata:
  type: project
---

State at handover (2026-10-20, all pushed):
- fluor 5487994: framebuffer = VSF RGB g2t 8-bit, darkness-inverted, Rec.2020 primaries (VSF RGB on macOS's ICC-tagged surface). White = code 194 (VSF truncation rule), 255 = 9.7× white.
- fluor::theme::vsf() const, integer Q16: authored g2t → working primaries → g2t buffer code, light above white kept.
- finalize = darkness flip + display map in ONE table per channel (theme::DARK_TO_SDR / to_sdr_visible; every finalize kernel, overlay pass, FPS strip). Today SDR: g2t → γ2 display code, above white clipped.
- γ2 sources enter via GAMMA2_TO_G2T / from_gamma2 / darkness_from_gamma2 (emoji, icon, photon 789dc60d: avatars, previews, viewer, beam video, wave field, vsf_rgb_to_stored, ANSI, orb ring, tints). photon theme.rs palette re-authored g2t in 3fafc971.
- Blending unchanged, on codes (Nick's choice). Matrices colorimetric (vsf 02076e6), greys stay Illuminant E.

Nick's constraints: keep the 8-bit g2t buffer, blending as-is, a final table; any matrix integer multiply-and-shift unless floats are proven necessary AND faster.

Open, in order:
1. ANDROID (Nick's Pixel 8 Pro first): RGBA_F16 extended-range linear surface (1.0 = SDR white) + ASurfaceTransaction_setExtendedRangeBrightness; probably an ASurfaceControl child + CPU-lockable AHardwareBuffer (check AHardwareBuffer_isSupported; ANativeWindow_lock lists 8-bit only). Window COLOR_MODE_HDR + desiredHdrHeadroom ≈ 1.5. Headroom: Display.getHdrSdrRatio + listener (API 34, verify). Final table → g2t → half-float linear via vsf::colour::display::display_light(l, headroom, content_peak); rebuilt on headroom change. Open tests: CPU F16 writes on this phone; does extended range take BT.2020 primaries (else scRGB → integer 3×3 per damaged pixel). Never PQ for UI (absolute nits, ignores the slider). PhotonActivity.kt:506 says DISPLAY_P3 while the surface tag is BT.2020 γ2.2.
2. DARKNESS-0 AUDIT — DONE 2026-10-20 (fluor 1bed892, photon e3d679a8): fluor::theme::WHITE + white_at(α); hint, slider, orb glow, blinkey floor, photon watermarks + logo glow. Rule: white light = WHITE, never darkness 0.
3. macOS: CAMetalLayer wantsExtendedDynamicRangeContent + Rgba16Float wgpu surface (renderer_wgpu.rs is Bgra8Unorm) + NSScreen maximumExtendedDynamicRangeColorComponentValue.
4. Windows: UpdateLayeredWindow is 8-bit; HDR needs a DXGI FP16 scRGB swapchain presenter.
5. Linux: Wayland color-management where supported; X11 stays SDR.
6. fluor::theme::set_display_color_space (JNI) is set but read by nothing.

Refs: vsf idt/output.md "Peak brightness depends on how much is lit", colour.md "Display Mapping"; vsf colour::display (display_light, display_lut_u8, g2t_peak, abl_*), colour::transfer (pq_light, hlg_light). The vsf session answers colour-side questions.

**Why:** Nick 2026-10-20: UI colours are VSF RGB g2t, the start of an HDR stack; he then handed the whole stack to the photon session.
**How to apply:** fluor is a photon dep (in scope). Device tests need Nick's phone; Android builds via scripts/android/dev-adb.sh. See [[project_theme_rec2020]], [[feedback_direct_pixel_no_floaters]].

REQUESTED HEADROOM (Nick 2026-10-20: "make sure we get our requested headroom right. I forget where 255 lands"): g2t code → light: 194 = 1×, 215 = 1.5×, 227 = 2×, 246 = 4×, 255 = 9.72× (bucket bottoms, fluor G2T_LIGHT_Q16). The request must follow the brightest light the frame USES, capped at the display's offer — never a fixed constant, never 9.7× just because the buffer can encode it.
