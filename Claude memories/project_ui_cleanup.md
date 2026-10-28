---
name: project_ui_cleanup
description: UI cleanup arc started 2026-10-28 with opsin's viewer panel — one height-budgeted unit, content-sized panel, fitted labels, height budget; steps 4-6 open
metadata:
  type: project
---

Nick 2026-10-28: "the opsin interface needs a lot of work … maybe this will be the start of the UI cleanup? … the actual layout, especially when adjusting the scale/RU is … not great … make sure it reads and is usable at all scales."

Diagnosis (opsin view.rs): three units mixed — panel width / navigator / histogram as area fractions (ignore ru), bands/pads/fonts linear in span·ru with no height budget, histogram pills sized off the hist rect; retained Buttons clipped labels; no vertical budget (chart collapsed, sliders off the bottom); the HUD unbounded vertically; Photon's Back/Save pills drawn over opsin's area unknown to it, and a duplicate Save.

DONE (steps 1-3): opsin a81471c + fluor db74037 (Button::set_font_size_fitted). band_of = s·h/(s+h), s = span·ru·3/128, h = area_h/16 (≈22 px at 1080p ru 1, 39 at ru 3); pad = band/2; panel_w = soft_min(soft_max(frac·w, 16 bands), w/2); every pill font fitted; histogram pills band-sized; panel_rects height budget (fixed rows first, nav/hist/chart share the rest). Verified offscreen with a throwaway harness (render View into a Canvas at sizes × ru, fluor::paint::finalize_for_os, dump raw BGRA, ImageMagick in scratch — never commit the harness or any encoder).

OPEN: (4) bound the HUD (vertical clamp + clip to the image area; long lines currently cut at the divider at high zoom in small windows) and keep the toast/hints from overlapping it; (5) opsin learns the host's reserved top band (Photon's Back/Save) so fit() avoids it, and one Save instead of two; Android safe-area insets on the panel; (6) carry the discipline (one budgeted unit, content-sized panels, fit-to-slot text, no fixed pixels) across Photon's screens.
**Why:** the layout is the thing users see; it must read at every scale.
**How to apply:** new UI derives every size from one budgeted unit (see SettingsLayout / opsin band_of); no area-fraction sizes for controls, no fixed pixels, labels fit their slots. Related: [[feedback_smoothness_for_humans]].
