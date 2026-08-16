---
name: project-window-geometry-parked
description: Window pos/size persistence PARKED 2026-08-16 - raw winit restore fought fluor's window model (flash-then-vanish, dead clicks); redesign = fluor-owned thru window_rect
metadata:
  type: project
---

**PARKED @e047a53 2026-08-16.** display.window.pos/.size (typed v_i5/v_u5 pairs, device-local) exist end to end — but the restore drove RAW winit calls (set_outer_position/request_inner_size) against a window FLUOR owns (oversized surface, drawn chrome, X11 input region, window_rect). A save flushed raw-OS values (captured during remote wmctrl testing, 300,220 · 753x1195) and every launch restored them → window flashed 2-3 frames, vanished, clicks fell thru a dead input region. Zoom (f5 scale) restore is UNAFFECTED and field-confirmed.

**The redesign (build this, not another patch):** geometry persistence becomes a FLUOR feature — save/restore the host's logical window_rect + surface placement thru the same machinery toggle_maximized uses (resize scratch, clip, input region, on_resize propagation). Photon keeps only the storage (the typed pairs + the event-edge dirty tracking, which is sound). The event hooks (on_window_moved/on_focus_changed, fluor bb5ba90) and the show-raise pulse (df13363) stay — they're correct regardless.

**Un-park checklist:** fluor apply-path + fluor-side geometry READ (what to save = window_rect, not raw winit); then flip take_window_geometry_request + flush_window_geometry back on; clear any poisoned pairs by first save.
