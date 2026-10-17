---
name: project_scroll_memmove
description: "Scroll as a memmove (2026-10-06, phone field-verified 10-07): fluor scroll_hint + memmove of the bg layer and hit map, paint only the band; settings pane not yet"
metadata:
  type: project
---

Nick 2026-10-06: "how do we make it so it's just a mem move like we do with moving the window?" Before: every scroll was a full viewport repaint (22 ms/frame on desktop).
**Shape:**
- fluor: the desktop host and Android shell memmove SCRATCH on `FluorApp::scroll_hint` BEFORE `clear_scratch_rect`; `DefaultChrome::scroll_bg(rect, dy, band, …)` shifts the bg layer and re-rasters only the band; `scroll_hit_map(rect, dy)`.
- photon: `pane_scroll` accumulates `scroll_shift` while the offset stays in bounds and skips invalidate_bg/chrome/scene_dirty; `damage_rect` arms `scroll_hint_armed = (rect, dy, band)` from `last_pane` when nothing else is dirty (damage = band ∪ fixed overlays ∪ their shifted copies); otherwise a shift forces a FULL repaint. The hint is valid only when the host didn't do a full repaint; if a shift happened but no hint carried the frame, render invalidates bg + chrome itself.
- Panes: the conversation list and the contacts block. Not done: the settings content pane.
**Convictions (rules):**
- The wheel is exempt from the blanket `scene_dirty` claim in `on_event` (its consumers mark their own).
- Anything that wants a frame WITHOUT a full repaint must not go thru `needs_redraw` (tick folds it into scene_dirty) — the fling calls `request_redraw()` directly.
- The fling ends at a bound (`pane_in_bounds`); samples are per nominal frame; ≤4 trailing still frames are dropped before the pick, 5+ = a hold; `fling_pick` = the largest of the newest three.
Result: memmove frames 6–11 ms on the phone, one settle frame after each glide. Dev instrument: `SCROLL:` lines.

**Why:** Nick: scrolling "is quite slow".
**How to apply:** keep the hint path self-protecting (a refused hint never leaves a half-applied scroll). Related: [[project_ready_first_paint]], [[feedback_render_never_touches_vault]].
