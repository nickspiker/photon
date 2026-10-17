---
name: project_ready_first_paint
description: "v110 (2026-10-01): content hit stamps ride the paint decision (stamp only where nothing opaque yet), never re-stamp the orb; a dirty flag raised mid-render must survive that render"
metadata:
  type: project
---

Two Ready-screen first-paint bugs (Nick 2026-10-01), both fixed.
**Orb unclickable until scroll:** content composites UNDER the chrome, but hit stamps were blind fills after painting, so a row under the title strip took the orb's pixels. Bandages (re-stamp the orb last; a geometric floor) were tried and REMOVED.
Rule: a content hit stamp runs BEFORE its content paints and writes only where `target[i] < 0xFF000000` — `stamp_hit_rect_under` / `stamp_hit_circle_under` (photon), and fluor `Button::draw_pill_immediate` likewise. Blind `restamp_hit_rect` is only for clears (HIT_NONE) and deliberate after-paint re-wins. A screen swap invalidates chrome.
**Contacts under the wrapped filter strip until scroll:** the measuring frame raised `scene_dirty`, the end of the same render_frame cleared it, and nobody requested a redraw. Fix: the reset moved to the START of render_frame + `request_redraw()` beside the measurement.

**Why:** "when we paint under we get a bool; if we never painted that pixel because something was there, skip the hitmap" (Nick).
**How to apply:** hit stamps follow paint ownership, never geometry; a dirty flag raised inside render must not be cleared at the end of that render.
