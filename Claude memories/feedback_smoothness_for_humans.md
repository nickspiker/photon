---
name: feedback-smoothness-for-humans
description: "Smooth math in Photon's GUI exists because humans physically prefer continuity — not for \"technical correctness\" or storage-format reasons."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: a9f51934-7668-45d4-b896-93ebab974b35
---

When asked why fluor / Photon insists on C¹-or-better continuous derivatives for GUI elements (curves, animations, layout transitions), **the reason is human perception, not technical correctness**. Humans feel derivative discontinuities the way they feel road potholes — even when they can't consciously see them. A clamp, a piecewise `if`, a circular arc joined to a straight line, or any other `C⁰` join registers as a micro-slap to the visual cortex. Smooth = massage; discontinuous = slap.

**Why:** the user pointed this out after I tried to justify the "calculus or else" rule with framings about 8-bit precision, supersampling cracks, and analytical correctness — all of which are real but **secondary**. The primary motivation is that products feel cheap when they're full of perceptual papercuts, even when each papercut is too small to consciously notice. Cumulative discomfort is the failure mode.

**How to apply:** when explaining why a piece of math is the way it is in Photon's UI / fluor's compositor — corners, easings, falloffs, layout interpolations, anything visible — lead with "humans feel it" rather than "the math is more correct" or "it survives precision changes." The math is correct *because* humans feel it, not the other way around. Same goes when defending a smooth choice against a tempting clamp or piecewise approximation in code review: the cost of the slap, not the cost of the math, is the argument.

Related: [[feedback-legacy-first]] (faithful porting first), `AGENT.md` § "GUI Continuity: Calculus or Else" (the formal rule).
