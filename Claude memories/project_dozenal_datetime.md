---
name: project-dozenal-datetime
description: "THE dozenal date/time render convention (canonical home = inksurf): month is a single zero-indexed glyph, days-of-week are words never digits, components separated by organic spaces (no punctuation)"
metadata: 
  node_type: memory
  type: project
  originSessionId: 4dcd0cb3-95b3-4d99-850b-f0d40f6ad308
---

Dozenal date/time convention, agreed 2026-09-01 (canonical convention lives in inksurf; photon renders to match).

- Date = `<year-glyphs> <month-glyph> <day-glyphs>` — separated by plain SPACES, never dots/dashes/slashes ("space it organically"). Components self-identify by shape: year is wide, month is always exactly ONE glyph, day is 1-2 glyphs.
- Months are ZERO-INDEXED single dozenal glyphs (a dozen months maps exactly): January = Zil (0) … December = Stelor (11); February = "a short Zila".
- Day-of-month is 1-indexed dozenal glyphs (the 31st = 27doz, max two glyphs).
- Day-of-week is a NAME, never a number — seven is prime and coprime to twelve, no glyph mapping exists; everything that counts in twelves gets glyphs, everything that counts in sevens gets words. If a machine ordinal is unavoidable (recurrence rules, sort keys), zero-index Monday = Zil … Sunday = Lun and keep it OFF human-facing surfaces.
- Week-of-year: dropped entirely, never rendered.
- Rejected: the 6-day half-dozen week (pretty, but breaks cadence interop with everyone else's lived calendar — notation can be opinionated, cadence must interoperate).
- As always: binary at rest, base chosen at the render edge only ([[feedback-numbers-binary-at-rest]]); glyphs via the Oxanium `+glyphs` face (0x10..0x1B), words via [[feedback-voca-camelcase]] voca for read-aloud.
