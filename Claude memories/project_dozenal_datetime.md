---
name: project-dozenal-datetime
description: "Dozenal date/time render: `year month-glyph day weekday .share`; month a single zero-indexed glyph, weekdays are words, spaces not punctuation; time of day = fixed-width share of today; hex = seconds since midnight / raw stamp"
metadata:
  type: project
---

SCOPE (Nick 2026-10-17): this is the render when the user's Base setting is dozenal; the app's default display is Arabic ([[feedback_numbers_binary_at_rest]]).
Convention agreed 2026-09-01 (canonical in inksurf; photon renders to match):
- Date = `<year-glyphs> <month-glyph> <day-glyphs>`, plain SPACES, never dots/dashes/slashes. Month = ONE zero-indexed glyph (January Zil … December Stelor); day-of-month 1-indexed glyphs.
- Day-of-week is a NAME, never a number (seven is coprime to twelve): twelves get glyphs, sevens get words. Week-of-year never rendered. The 6-day week was rejected (cadence must interoperate).
- Time of day (2026-10-01, matches inksurf's tide clock) = a SHARE of today: `.` then digits, Zil midnight, Lun noon, each digit a twelfth of the one before; FIXED width (four digits for a stamp, six for the live Base clock). The point is the only separator.
- Example: `ZilaZilorZilStela Stel Zila Thursday .TeraLunorLunZil` = Thu 2026-10-01 09:25:00. The whole line every time under a message (a bare share read as a decimal).
- Hex: clock = seconds since local midnight; a stamp = raw oscillations since the Eagle epoch. Arabic: wall clock.
Code: `day_share_glyphs`, `fmt_clock`, `fmt_when` in src/lib.rs.
**How to apply:** binary at rest, base at the render edge only ([[feedback_numbers_binary_at_rest]]); glyphs via the Oxanium face, read-aloud via voca ([[feedback_voca_camelcase]]).
