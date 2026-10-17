---
name: feedback-smoothness-for-humans
description: "Smooth (C¹+) math in Photon's GUI exists because humans physically feel discontinuities — lead with perception, not technical correctness"
metadata:
  type: feedback
---

Fluor / Photon insist on C¹-or-better continuity for visible elements (curves, animations, layout transitions) because **humans feel derivative discontinuities** like road potholes, even unseen. A clamp, a piecewise `if`, an arc joined to a line — any C⁰ join — is a micro-slap; smooth is a massage.

**Why:** Nick corrected me when I justified "calculus or else" with 8-bit precision, supersampling cracks and analytical correctness — real but secondary. Products feel cheap when full of perceptual papercuts too small to notice individually; cumulative discomfort is the failure mode.

**How to apply:** when explaining or defending smooth math (corners, easings, falloffs, interpolations), lead with "humans feel it"; the cost of the slap, not the cost of the math, is the argument against a tempting clamp. Formal rule: AGENT.md § "GUI Continuity: Calculus or Else".
