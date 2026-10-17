---
name: project-window-geometry-shipped
description: "Window geometry persistence SHIPPED 2026-08-16 thru fluor's window model (apply_window_rect + once-per-gesture on_window_rect_changed); launch-order doctrine"
metadata:
  type: project
---

SHIPPED 2026-08-16 (fluor 345ed36 + 257fb46, photon same day).
- fluor `apply_window_rect` is the ONE programmatic placement path; the restore request is applied only once the home surface has real geometry, clamped to surfaces.
- fluor `on_window_rect_changed(x,y,w,h)` fires ONCE per settled user gesture (never while maximized) in global desktop units; photon saves immediately there — the gesture is the durability edge, no dirty tracking or timers.
- Storage: typed device-local keys ([[project_settings_typed_values]]).
- `WindowHandle::outer_position/inner_size` were REMOVED from fluor: they read the fullscreen surface, not the visible window.
- Placement requests are consumed at the top of home `RedrawRequested`, before render, so frame one is already placed (fixed a launch flash).

**Launch order doctrine (Nick):** binary hash self-check → vault → window specifics + contacts → render → avatars/heavy work.
