---
name: project_macos_resize_cursor
description: "macOS resize arrows intermittent (2026-09-18): fluor's click-thru decision + re-entry monitor tested the BARE window rect while the input region + edge classifier used the band-inflated one, and re-entry was a redraw-poll nobody started; FIXED — one hittable_rect, entry-edge wake. Field test PENDING."
metadata:
  type: project
---

macOS resize arrows showed intermittently (Nick 2026-09-18). Root cause lived entirely in fluor's macOS click-thru layer (the monitor-sized transparent surface + `ignoresMouseEvents`), not photon, and not the platform.

Three geometries disagreed. `push_input_region` inflated the hittable region by the resize band (strip_height/4) and `get_resize_edge` classified just-outside coords as an edge, but `update_macos_hittest` and the global `HittestMonitor` tested the BARE `window_rect`. So on macOS the outer band flipped the surface click-thru the pixel the cursor crossed the hairline — cursor forced to the arrow, CursorMoved returned before `cursor_for`. Arrows only ever existed in the inner ~11 pt band; "intermittent" = which side of the line the hand stopped on. Linux unaffected (the X11 region is the only gate).

Re-entry was a poll nobody started. Once click-thru, macOS delivers no cursor moves; the global monitor set a flag that was only read in RedrawRequested, which kept itself alive by re-requesting a redraw each vsync — but nothing requested the FIRST redraw at the OFF flip, and the idle loop only redraws if `tick` asks. Leaving across a hovered widget (un-hover repaint) started the poll; leaving across plain background didn't, and the window stayed click-thru and cursor-blind until an incidental redraw (a click in that state passed thru to the app behind). While it ran, the poll burned a frame per vsync.

FIX (fluor, 2026-09-18): `hittable_rect()` = window_rect inflated by the band, the ONE definition used by the input region, the click-thru decision and the monitor's `update_rect`. The monitor now wakes the loop on the ENTRY EDGE — one `request_redraw` on the armed home window (`HittestMonitor::arm` at the OFF flip, which also clears the stale flag) — and RedrawRequested just consumes the flag. Edge, not timer ([[edges-not-timers]]). Field test PENDING.

Not fixed, platform behaviour: winit's macOS `set_cursor` is `invalidateCursorRectsForView`, and AppKit honours cursor rects only for the KEY window — hovering an unfocused photon window's edge keeps the arrow. A real per-window `NSWindow` would get system edge tracking (works on inactive windows too) at the cost of a macOS-only window model; fluor's monitor-sized surface is the portable "draw anywhere" (every compositor is rect-bound; X11 override-redirect is the odd one). Product call, not a bug.

**Why:** three places each re-derived "is the cursor on the window" and drifted; a poll masked its own missing kick.
**How to apply:** any new hit/cursor/click-thru logic derives from `hittable_rect()`; cross-thread signals into the loop are one wake on the edge, never a self-sustaining redraw.
