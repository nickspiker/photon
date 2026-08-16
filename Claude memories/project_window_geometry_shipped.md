---
name: project-window-geometry-shipped
description: Window geometry persistence SHIPPED 2026-08-16 thru fluor's window model (apply_window_rect + once-per-gesture settle hook); raw-winit park resolved by design
metadata:
  type: project
---

**SHIPPED 2026-08-16 (fluor 345ed36 + photon same-day).** Window pos/size persist and restore thru fluor's OWN window model — the raw-winit park (@e047a53: flash-then-vanish, dead clicks from moving the fullscreen OS surface) resolved by design, not patched.

**The shape:**
- fluor `apply_window_rect` = toggle_maximized's tail extracted — the ONE programmatic placement path (viewport/scratch/clip rebuild, consumer on_resize, input region, repaint). `take_window_geometry_request` applies thru it, clamped via clamp_rect_to_surfaces, taken only once the home surface has real geometry so the one-shot can't burn on placeholder dimensions.
- fluor `on_window_rect_changed(x,y,w,h)` fires ONCE per settled user gesture (drag-move release, resize-drag end; never while maximized — a mode, not a placement). GLOBAL desktop units, the same currency the restore consumes.
- photon saves IMMEDIATELY in that hook — dirty tracking, flush edges (focus-lost/close), init seeding, on_window_moved all DELETED. The gesture is the durability edge ([[edges-not-timers]], once per gesture so no write storms).
- Storage unchanged: display.window.pos v_i5[x,y] + .size v_u5[w,h], device-local unlinked, fleet-mirrored like zoom ([[project-settings-typed-values]]). Poisoned old pairs are clamped at apply and overwritten by the first real drag.
- `WindowHandle::outer_position/inner_size` REMOVED from fluor — they read the fullscreen surface, not the visible window; the trap is fenced at the API layer.

Field verify pending: restore-on-launch + move/resize round trip on Nick's desktop.
