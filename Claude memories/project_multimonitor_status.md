---
name: project-multimonitor-status
description: "fluor multi-monitor phases A/B/C-core shipped; macOS real-test FAILED (window vanishes on cross-monitor drag, pinned); Linux black-taskbar = Muffin unredirect, gsettings workaround on dev box"
metadata: 
  node_type: memory
  type: project
  originSessionId: bf3c2e39-d57b-4469-8848-1780b1b5c927
---

fluor multi-monitor (per-output surfaces, plan @ ~/.claude/plans/it-s-been-committed-then-quiet-babbage.md): phases A (surface Vec) + B (multi-surface spawn/route/dormancy) + C-core (RasterPass dual-raster, settle rebase) + macOS port SHIPPED in fluor; phase D (rotation/hotplug) + Windows layered port NOT built.
macOS real-hardware test FAILED 2026-07-25: window DISAPPEARS when dragged toward another monitor — PINNED per user (ticket in photon TICKETS.md, fluor-side section). Suspects: second surface never spawning, involved()/dormancy flip hiding home, points-vs-pixels in straddle math.
Linux black-taskbar FIXED 2026-07-25 @ fluor b2b8907: TRUE root cause = Muffin auto-promotes an undecorated EXACTLY-monitor-sized window to legacy FULLSCREEN state (fullscreen layer buries the panel). Unredirect + bypass-compositor were red herrings (gsettings flip reverted). Fix = surface height−1 on Linux (create_monitor_surface); verified live via xprop + screenshot; photon also gains a taskbar button. Don't "fix" the −1 back to exact monitor size.

**2026-10-22 Linux screen-change fix (fluor ba4996b, unverified on hardware):** Nick: after monitor plug/unplug/resolution change Photon painted the screen black or wouldn't appear until relaunched. Two causes from code: (1) rebuild_span read winit's CACHED monitor list (winit invalidates only when its own connection handles RRScreenChangeNotify) right after fluor's connection drained the same notice → "nothing moved", never re-checked; now x11_atomic::monitor_layout() (RRGetMonitors on fluor's connection). (2) the WM constrains the oversized span to exactly monitor size → Muffin legacy-fullscreen → unredirected → black; handle_surface_resized now pushes back once per layout (span_pushed_back). Verify via FLUOR-MON lines ("layout changed", "putting it back") in the next log after a screen change.
