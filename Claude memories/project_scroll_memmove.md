---
name: project_scroll_memmove
description: "2026-10-06 SCROLL AS A MEMMOVE built (conversation list + contacts block): fluor already had scroll_copy_rect + the scroll_hint hook (unused); photon arms (rect, dy, band) in damage_rect, hosts memmove scratch (+ persistent screen on desktop), render memmoves bg layer + hit map and paints only the band + fixed overlays; settings pane not yet; field verify pending"
metadata:
  type: project
---

**Nick 2026-10-06:** "scrolling itself is quite slow … how do we make it so it's just a mem move like we do with moving the window when the user scrolls?" → "Go for it!"

**Why it was slow:** every scroll set `scene_dirty` → `damage_rect` = full viewport → every visible row re-rasterised, the bg noise layer re-rastered whole (`invalidate_bg`), the chrome layer + hit map re-stamped (`invalidate_chrome`), full finalize + present. 22 ms/frame on the desktop Ready screen.

**Shape (shipped):**
- fluor had `paint::scroll_copy_rect` and the `FluorApp::scroll_hint` hook with the desktop host memmoving `persistent_screen` — photon never implemented the hook. Added: the desktop host AND the Android shell memmove SCRATCH on the hint BEFORE `clear_scratch_rect` (the clear would zero the memmove's source band); `DefaultChrome::scroll_bg(rect, dy, band, damage, paint)` (bg layer shift + band-only re-raster, layer stays clean) and `scroll_hit_map(rect, dy)`.
- photon: `pane_scroll` accumulates `scroll_shift` (+ = content down) only while the offset stays inside [0, max] (Ready: `old − new`; Conversation: `new − old`, and only when the blind did NOT move) and then skips `invalidate_bg/chrome` and `scene_dirty`; `damage_rect` (idempotent) arms `scroll_hint_armed = (rect, dy, band)` from `last_pane` when nothing else is dirty, damage = band ∪ each fixed overlay ∪ its copy shifted by dy ∪ widget damage; otherwise a shift forces a FULL repaint (never a skipped one). Render: hint valid only when `ctx.damage_clip != full viewport` (a host full repaint did no memmove); bg via `scroll_bg` with the band as clip (noise_split + version text take the clip); hit map shift after `rasterize_chrome`; the conversation `scroll` is ROUNDED; `list_clip`/`rows_clip` narrowed to the damage (`clip_to_damage`); `flatten_bg_into` clipped to the damage; `last_pane` recorded per frame (1 = conversation list rect [list_top, list_bottom) with toast + orb overlays; 2 = contacts block [strip_height or 0, buf_h) with standing bands + orb); shift + hint cleared at render end.
- Correctness lean: rows outside the band are still walked (hit rows, stamps — `stamp_hit_rect_under` skips opaque pixels, so memmoved rows keep their memmoved stamps); painting outside the damage is only wasted work, never wrong.

**Not done:** settings content pane (rail fixed beside, split bg per pane); mixed-DPI extra pass may overwrite `last_pane`. Field verify on a phone pending (seams, overlays, PERF lines).

Related: [[feedback_render_never_touches_vault]], [[project_render_storm_lag]], [[project_ready_first_paint]].

**First phone build CONVICTED 2026-10-06 (Nick: "has not been applied… seems more glitchy"; phone log: every scroll frame still 17-23 ms):** the pre-dispatch rule in `on_event` set `scene_dirty` for EVERY event except CursorMoved — the wheel (and Android's synthetic touch wheel) included — so no frame ever qualified for the hint, while `pane_scroll`'s rigid path had already skipped `invalidate_bg/chrome` → the bg noise stood still under moving rows and hit stamps lagged a shift. Fixes: the wheel is exempt from the blanket claim (its consumers mark their own); `row_hover` does not dirty the scene while `press_held`; the render invalidates bg + chrome itself whenever `scroll_shift != 0` but no hint carried the frame (self-protecting — a refused hint can never leave a half-applied scroll again).
