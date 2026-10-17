---
name: project-hotplug-abort
description: "2026-10-15 monitor-hotplug: photon v119 SIGABRT twice (12:06, 13:13) at the avatar resizer expect (0-px destination); FIXED in 0.120.1 dev, NOT in v120; per-release symbols still wanted"
metadata:
  type: project
---

2026-10-15 on leviathan (Linux/X11 Cinnamon): plugging a monitor coincided with the session losing taskbar/apps and two hard resets; photon-messenger (v119) SIGABRTed at 12:06 and 13:13, with rustdesk aborting alongside both times.
Core symbolization FAILED: the installed v119 binary is stripped, no unstripped build with its BuildID existed, and /tmp (photon.crash.txt + tmpfs log) died with the reset.
CONVICTED by repetition: both cores hold "…vatar_render.rs:23:6" = the resizer `.expect` in update_avatar_scaled — resize 0.8 returns InvalidParameters for a 0 destination, and callers compute `(radius * 2.0) as usize`, which hits 0 at a squeezed viewport (X dropped to a 320×200 framebuffer with every output off, then returned at 5760×2130).
FIXED same day: zero diameter → empty texture, draw_avatar refuses a short texture, test a_zero_pixel_avatar_is_empty_not_a_panic. fluor `rebuild_span` / `resumed` already return on an empty monitor list.
Desktop side: the "internet down" was upstream DNS; the Cinnamon fallback matches the rustdesk-fork fix ed56b73d9 (XEmbed tray icon) and the installed rustdesk predates it. Rustdesk is not ours to edit from photon threads — tell Nick, don't touch.

**Why:** Nick: "If I unplug a monitor and my machine crashes, that's a fucking problem" — photon must survive a zero-monitor / tiny-framebuffer moment.
**How to apply:** make deploy keep per-release debug symbols (objcopy --only-keep-debug keyed by BuildID) so the next core symbolizes; any size-derived allocation must tolerate 0; reproduce with Xvfb + `xrandr --fb 320x200` under an isolated HOME (never disturb the live instance).
