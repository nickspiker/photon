---
name: project_black_span_after_resolution
description: 2026-10-27 CONVICTED + FIXED in fluor: after a monitor/resolution change Photon's spanning X11 surface stayed unpainted (black over both monitors, unclosable) — the softbuffer buffer was never resized
metadata:
  type: project
---

Nick 2026-10-27: "opening Photon freezes the entire GUI … every time I change my screen resolution … literally impossible to close" (the window never draws, taskbar menus and xkill useless). Reproduced by Nick: "Nice black screen when you click on Photon, it's open and hiding everything."

His event: DisplayPort-0 drops out, HDMI-A-0 goes fleet-2560x1600 → 3840x2160, DisplayPort comes back; fluor's single spanning surface follows to 5760×2159.

Cause (fluor src/host/app.rs): the monitor-layout path sets `s.size` to the new span BEFORE `request_inner_size`, so the Resized that follows matched `s.size` and returned before `surface.resize` — softbuffer stayed at the old size, and every present then skipped silently on `buffer.len() != persistent_screen.len()` ("the resize repaints" — it never did). The grown window was never painted: `import -window` showed all 12,435,840 pixels zero, X shown it as opaque black over everything.
Not the cause (ruled out with probes): X flooding (xprop stayed at 3 ms), input-shape loss (pointer windows correct), photon-monitor-watch, a pointer grab.

Fix: every CPU present (home, raw, drag shift) sizes the softbuffer surface to the surface's backing first (softbuffer's resize is a no-op at an unchanged size), so no path can strand the buffer.

**Why:** a stale present buffer turned the invisible spanning surface into an opaque, unclosable black wall.
**How to apply:** any new present path must size its platform buffer to backing before writing; never "skip and wait for a resize" without an edge that guarantees one. Related: [[project_multimonitor_status]].
