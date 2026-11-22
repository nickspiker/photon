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

**2026-11-21 (photon 6000c63a, fluor 28251c9):** a56ca063's one noise field makes a memmove frame redraw everything outside the pane, so the rects' box is the whole frame; the ferros and Android hosts took a hint only for a part box, so on the Pro nothing rode the memmove and the pane went unconverted. Rule now: hosts take the hint unless the frame is ONE whole rect, and say so in `Context::scroll`; the render reads `ctx.scroll`, never the damage's shape. Pane clips narrow to the rects that meet them (not their box). On a safe circle the unseen blind no longer refuses the memmove. Fills clamped at a list edge floor each edge on its own (a floored height dropped the last row; the memmove carried it up as a line).
**The rig:** `cargo test --lib scroll_rig -- --ignored --nocapture --test-threads=1` (src/ui/photon_app/driver/scroll_rig.rs): Pro size, safe circle, a fling drawn as the ferros host draws it, each frame held to the frame drawn whole. Floor of a ferros conversation memmove frame: ~34 % drawn (band + header + compose bar under the moving noise) until the GPU/pane-composite model. Open: the contacts' filter-slat pills land ~1 px off a pure move while passing under the search box.
