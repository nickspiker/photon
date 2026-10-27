---
name: project_beam_standing_queue
description: 2026-10-27 Nick↔Jon laggy beam: the beam verdict read a FULL queue and a blackout as clean — fixed (standing queue + silent second = congested); Jon on v0.119
metadata:
  type: project
---

Field 2026-10-27 22:19–22:30 UTC, Nick's MacBook (fe46a74b, current) ↔ Jon's Pixel (7d23ad30, v0.119.0 — 13 releases behind), over the internet (Nick 75.253.157.169, Jon 174.45.252.101).

What the logs showed: the audio measured the path at 40–110 kbit/s (rtt-slope capacity), Jon received the Mac's audio with 10–40% loss, RTT swung 64 ms → 3–7.5 s with ~40 s of no samples at all. Jon's v0.119 phone ladder still climbed to 2560×1920 @ 7.4 Mbit/s and 1280×960 @ 1.8 Mbit/s. The Mac beam held 1280×720 soft 0 @ 30 fps (~2.7 Mbit/s actual — about 2× its 1382 kbit/s plan) for three minutes without one step down; Jon showed 4912/8429 pictures concealed.

Cause (engine.rs, the beam's LINK_VERDICT): congestion was loss-with-queue or a GROWING delay slope only. A filled queue (flat at seconds over a 64 ms floor) read calm → clean; a blackout (no RTT samples) left the slope at 0 → clean. So the beam held/climbed exactly while the link drowned.
Fix: standing_queue (rtt_ema − rtt_min > LOSS_BLOAT_CORROBORATION_MS, 100 ms) or blacked_out (no RTT sample this verdict second, once one has arrived) is congested and never clean. Audio tier logic untouched.

OPEN: the Mac's actual beam rate ran ~2× its plan (VideoToolbox overshoot or overhead in the "sent KB" count) — unverified; Jon must update (v0.119 has neither fix nor the Beam page caps).
**How to apply:** a link verdict must treat "no news" and "high but flat delay" as bad news; the ladder can only shed what the verdict convicts. Related: [[project_beams]], [[project_beam_settings]].
