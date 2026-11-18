---
name: feedback_ui_scale_range
description: "layouts must hold from 25% to 400% UI scale (ru); mockups are pixel sketches, the real layout is relative"
metadata:
  node_type: memory
  type: feedback
  originSessionId: 17b90e8e-8b1a-448e-8ac7-22b057e9a0d7
  modified: 2026-11-18T16:40:06.788Z
---

The person can set the UI scale (ru) from about 25% to 400%; some elements scale with ru, others shift to make room.
Owner, 2026-11-18, of the ferros Photon mockups: "it's not a pixel layout so it looks good in your sketches but take care when actually laying it out".
**Why:** a design that only works at 100% breaks for anyone who zooms; fixed pixels are already forbidden (AGENT.md "Dimensional Units").
**How to apply:** when turning a mockup into code, derive every position from the circle (cx, cy, r) and the ru-scaled unit, and check the extremes (25% and 400%) for collisions, such as the orb's shoulder beads against ferros's console circles; resolve crowding with smooth (C¹+) functions, never a clamp. Related: [[feedback_smoothness_for_humans]], [[project_ui_cleanup]].
