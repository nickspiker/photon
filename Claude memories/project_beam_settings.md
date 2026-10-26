---
name: project_beam_settings
description: 2026-10-26 Beam settings page + g2t HDR beams BUILT (per-device keys w/ follow-fleet, 5 live controls, test patterns, HLG10→g2t send on phones, g2t receive everywhere); field test pending
metadata:
  type: project
---

Nick 2026-10-26: "We need a beam settings screen to control bitrate/size/quality/HDRness (VSF RGB g2t) and a few other things… show some test gradients there too so we can check our HDR against 'white' and 'brightest' and see stairsteps for any banding/bitdepth".

Decisions (AskUserQuestion, 2026-10-26):
- Build the page AND the g2t stream in one round (receive first, then Android send).
- Settings default PER DEVICE, but "for most settings there will be a global link and unlink so the device can maintain its own settings OR sync with the fleet" — beam.* keys born unlinked, the page carries a follow-the-fleet toggle (fleet_settings set_link).
- Controls: g2t HDR on/off, max size, frame-rate cap, bitrate/quality, detail-vs-smoothness (what congestion sheds first).

Facts found (2026-10-26 map): no beam knob was user-settable (beam_ladder constants, bitrate_for 0.1 bit/px/frame, ceiling() in beam_session); beams carried no g2t (receiver clamps at white, PICTURES γ2, wave_field blits via from_gamma2); only Android shows HDR (fluor host/android/hdr_surface.rs, granted() private — needs a pub getter); no gradient/test-pattern primitive in fluor; slider widget exists but only the untested zoom slider uses it — pills are the proven segmented choice.

**Why:** HDR beams and a place to verify the HDR stack by eye (white vs brightest, banding stairs).
**How to apply:** see [[project_hdr_stack]] for the display side, [[project_beams]] for the beam chain; the page lives in the orb panel ([[feedback_orb_settings_panel]]).

BUILT 2026-10-26 (docs/beams.md §5f HDR, §5g Settings):
- Receive: beam_colour Converter now outputs g2t Rec.2020 (the buffer's encoding; wave_field blit no longer from_gamma2); light Q16, i64 matrix; SDR entries clip at white, Transfer::G2t keeps up to 9.7×. vsf Transfer::Gamma2Tanh "g2t" added.
- Send (Android): PhotonBeam hlgCapable/anyHlgCamera probe → nativeSetBeamHdrCapable; hdrNow = t.hlg && nativeBeamWantHdr at camera start → P010 reader + OutputConfiguration HLG10, camera's own colour; nativeBeamFillHdr → beam_hdr::fill_hlg_p010 (16-bit shrink, HLG→light OOTF γ1.2 per channel, 0.75 = white, LUT → g2t, dither, 601 YCbCr); hdr_entry = creative/assumed Rec2020→VSF, g2t. Self-view reads P010 high byte.
- Settings: beam_settings.rs (generation counter, follow_settings in beam_session retunes ladder live); ladder Tuning {fps_cap, bpp_milli, bitrate_cap (lowers top rung via raw_plan), keep_detail (FPS_FIRST=4 half-stops)}; keys beam.* device-local unless the page's follow toggle links them (cap = number | u0(false) Auto).
- Page: SettingsPage::Beam, pills only (slots 0-21, on_beam_pill), test patterns via beam_test_colour; HDR line from fluor hdr_surface::status() (new pub, Android only).
Field test: Pixel HDR on → log "request: hdr=true", "BEAM: phone sender up … HDR true"; check receiver highlights > white; check range tag (fullRange) is right by eye.
