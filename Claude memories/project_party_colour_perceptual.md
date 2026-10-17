---
name: project_party_colour_perceptual
description: "CLOSED 2026-10-06: party colours = uniform-by-area draw over the Y = 0.5 LINEAR slice of (VSF cube ∩ Rec.2020); 50% means physical, not L*"
metadata:
  type: project
---

`party_colour` gives each party a deterministic colour from its hash.
Ruling (Nick 2026-10-06): 50% means PHYSICAL — half the illuminant-E power reflected — so the Y = 0.5 linear plane stays and L* is not the frame.
The draw is uniform BY AREA over the Y = 0.5 slice of (VSF cube ∩ Rec.2020 cube) via digest-deterministic rejection sampling, replacing a hue-ray scheme that pushed every colour to the gamut wall.

**Why:** Nick: "not sure why we are clipping some to the edge and skewing away from grey".
**How to apply:** don't reintroduce perceptual-lightness (L*) or hue-ray schemes; colour output goes thru the Rec.2020 pipeline ([[project_theme_rec2020]]).
