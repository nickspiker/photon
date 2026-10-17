---
name: project_macos_resize_cursor
description: "macOS resize arrows intermittent (2026-09-18): FIXED in fluor — one hittable_rect for input region, click-thru and monitor; re-entry wakes on the entry edge; field test pending"
metadata:
  type: project
---

Root cause (fluor's macOS click-thru layer, 2026-09-18): `push_input_region` and `get_resize_edge` used the window rect inflated by the resize band, but `update_macos_hittest` and `HittestMonitor` tested the BARE rect — so the outer band flipped the surface click-thru and forced the arrow cursor.
Re-entry was a self-sustaining redraw poll that nothing kicked at the OFF flip, so the window could stay click-thru and cursor-blind.
Fix: `hittable_rect()` is the ONE definition (input region, click-thru decision, the monitor's `update_rect`); `HittestMonitor::arm` at the OFF flip and one `request_redraw` on the entry edge. Field test pending.
Platform limit, not a bug: AppKit honours cursor rects only for the KEY window, so an unfocused window's edge keeps the arrow (a per-window NSWindow model would fix it — a product call).

**Why:** three places each re-derived "is the cursor on the window" and drifted; a poll masked its own missing kick.
**How to apply:** new hit/cursor/click-thru logic derives from `hittable_rect()`; cross-thread signals into the loop are one wake on the edge ([[edges-not-timers]]).
