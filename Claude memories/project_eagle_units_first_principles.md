---
name: project_eagle_units_first_principles
description: "Eagle units: one anchor (hydrogen line, c=ħ=k_B=1), time's Zil = ONE oscillation (no display offset), mass = protium atoms, floor rule, log-form reputation proposed; Base page lesson shipped v110 (English only); docs/dozenal.md canonical"
metadata:
  type: project
---

**Principle:** DMS is a logarithm, so a unit is only where Zil sits; an offset by whole doublings is an addend, never a factor. "How much is a log, where is a share" (time of day stays a share because a day is a cycle).
**One anchor:** the hydrogen line with c = ħ = k_B = 1 — time = one oscillation, length = one wavelength, temperature absolute on the line photon (freezing Stelor, room Zila Zil), velocity absolute (c = Zil). Mass = how many ground-state PROTIUM atoms (the atom = 2^47.18 line photons, the one measured non-integer constant). Information: bit = Zil, byte = Ter.
**Decided 2026-10-01 (Nick: "1 second is not Zil"):** the Eagle second (SI in disguise) is retired as a unit; `dms_age` takes OSCILLATIONS, floor glyphs only; every scale offset is zero; `link_rtt_label` is a duration. Clock and epoch never move ([[feedback_epoch_fixed]]) — display-side only.
**Base page (v110):** pills Arabic → Hex → Dozenal (dozenal default), explain-don't-argue, say "arabic" not "base ten". ENGLISH ONLY: the other 15 language files route the new Msg variants to en.rs thru explicit arms after each file's WhyDozenalProse arm — a translation pass = fill those arms. Live digits wake on `live_digit_edge()` (driver.rs), never a timer.
**Reputation in log form (proposed, Nick: "math needs sorted"):** r = log2((P+1)/(N+1)) signed doublings over distinct people, support s = log2(P+N+1); a spotless record's reputation IS its evidence; the page's share-form RepOne/RepFill still need reworking.
**Marks:** dozenal radix = raised dot U+00B7 (`DOZENAL_POINT`); signed doubling counts ALWAYS show an arrow (↑ at/above the unit, ↓ below, `DMS_UP`/`DMS_DOWN`, from Noto Symbols); unsigned scales none. Rejected: "−" (Zil looks like a dash), "|" and "/" (read as Zila).
**Grammar:** "Eagle" names the frame like "metric"; quantity word + digits, never "12 eagles".
OPEN: Nick said "below each message live" — built in the tap-to-select details strip; ask. See [[project_dms_age]], [[project_numeral_forms]].
