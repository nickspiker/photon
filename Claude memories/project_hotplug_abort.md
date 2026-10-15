---
name: project-hotplug-abort
description: "2026-10-15 desktop monitor-hotplug: photon (v119) + rustdesk both SIGABRT at 12:06:46; core kept but unsymbolizable (stripped, no matching unstripped build); fix needs per-release symbols first"
metadata:
  node_type: memory
  type: project
  originSessionId: 34edabcd-24d7-46f8-8a4d-3aec382dfda0
  modified: 2026-10-15T19:48:58.073Z
---

2026-10-15 12:06:46 PDT on leviathan (Linux/X11 Cinnamon): internet dropped, Nick plugged a monitor into the physical port, the session lost taskbar/icons/apps, hard reset at 12:09:51.
`coredumpctl list` shows rustdesk (pid 6405, no core) and photon-messenger (pid 4438, core PRESENT, 87 MB) both SIGABRT within 2 s — photon's main thread: a Rust panic → abort (frames #7–#15 panic machinery, #16+ app code).
Core symbolization FAILED: installed binary BuildID 4b98624780532e39680e2945649e6ece45cf72a0 (v119, installed 04:23) is stripped and no unstripped build with that id exists; /tmp (photon.crash.txt + tmpfs log) was wiped by the reset.
Heap fragments in the core hold "…vatar_render.rs:23:6" = the resizer `.expect` in update_avatar_scaled — resize 0.8 returns InvalidParameters for a 0 destination, and callers compute `(radius * 2.0) as usize` which hits 0 at a squeezed viewport. LIKELY CAUSE (circumstantial — the panic text is that very location, no symbols to prove the frame); FIXED same day: zero diameter → empty texture, draw_avatar refuses a short texture, test a_zero_pixel_avatar_is_empty_not_a_panic.
Core also holds "FLUOR-MON: EMPTY input region on live home surface 0".
fluor `rebuild_span` / `resumed` already return on an empty monitor list.

**Why:** Nick: "If I unplug a monitor and my machine crashes, that's a fucking problem" — photon must survive a zero-monitor / tiny-framebuffer moment.
**How to apply:** first make deploy keep per-release debug symbols (objcopy --only-keep-debug keyed by BuildID) so the next core symbolizes; check the avatar resizer for a 0 diameter at tiny viewports; reproduce with Xvfb + `xrandr --fb 320x200` under an isolated HOME (never disturb the live instance).
