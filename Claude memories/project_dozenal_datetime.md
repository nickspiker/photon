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

**2026-10-01, time of day (photon, matches inksurf's tide clock):** a SHARE of today — `.` then digits, Zil midnight, Lun noon, each digit a twelfth of the one before (2 h, 10 min, 50 s, 4.2 s, 0.35 s, 29 ms); FIXED width (a clock's trailing Zil is a position): four digits for a tapped stamp, six for the live Base-page clock. The point is the only separator; inside a number the glyphs run together (camelCase in the read-aloud form). Full line: `ZilaZilorZilStela Stel Zila Thursday .TeraLunorLunZil` = Thu 2026-10-01 09:25:00. NO elision under a message (Nick 2026-10-01: a bare share on its own line read as a decimal with no context) — the whole line every time. Hex: clock = seconds since local midnight in hex (83E0 = 9:20:00); a stamp = raw oscillations since the Eagle epoch in hex. Arabic: wall clock, same elision. Code: `day_share_glyphs`, `fmt_clock`, `fmt_when` in src/lib.rs.
