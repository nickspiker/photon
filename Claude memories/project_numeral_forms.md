---
name: project_numeral_forms
description: "THE NUMBER FORMS (2026-09-11): counts linear, magnitudes DMS = floor(log2)+1 spelled dozenal, proportions one dot-fraction digit; hex linear everywhere (seconds/bits/ms/mm); anchors per scale (bit, Eagle second, hertz, hydrogen wavelength 21.1 cm); docs/dozenal.md"
metadata:
  type: project
---

Decided with Nick 2026-09-11 and shipped in photon (docs/dozenal.md is the canonical text):
- **Three forms by kind of number, not by screen**: counts (peers, unread, chunk 3 of 5) are linear digits in the base; magnitudes (size, age, rate, length) are DMS; proportions (battery, rung, vote share) are one dozenal fraction digit with a dot (.Lun = half, .Stelor nearly whole). Identifiers never convert. Reputation, when built, is signed doublings from the median peer, not a rating.
- **DMS = "Dozenal Metric Scaling": doublings, spelled dozenal.** It is floor(log2 x) exactly, written in base twelve (the +1 bit-length form was dropped 2026-09-11 evening: Nick "should it really be Zil? Not Zila"); zero reads as a word (now / empty) — Nick: "just straight up log base something conversion, the only evenly spaced scale on this slide rule". "Metric" is the doubling; "dozenal" is only the numeral.
- **Unit of account (the "1" of each scale)**: a bit (size), an Eagle second (age), one hertz (a link is inverted so its floor is one), one hydrogen-line wavelength = c / vsf::OSCILLATIONS_PER_SECOND = 21.106 cm (length; `HYDROGEN_LINE_METRES`, `dms_length`, `length_doublings`, `dms_doublings_glyphs/spell` in src/lib.rs). Below one only length uses a minus for halvings (−Zila a hand, −ZilaZil a hair); Planck length −StelLun, the unit Zil, a person Ter, the Moon and a light-second ZilorLun, the universe LunaLuna. Planck-anchored (all positive) was rejected: the human scale vanishes into two Stel digits.
- An hour is Stelor, a byte Ter, a megabyte ZilaStelor. **Hex is linear everywhere**: timers as plain seconds (fmt_duration_secs hex arm), RTT in seconds with a hex fraction (hex_seconds_ms: 66 ms = 0.10E), sizes in BITS unitless (unit_size routes KiB/MiB sites thru dms_size), exposure stops thru `fmt_halves` (.8 hex, .6 dozenal, .5 arabic), the clutch ladder prefix thru fmt_num. Diagnostics record timestamps stay arabic clock time (correlation with photonlog/adb). Tests: `base_kat` (src/lib.rs, a mutex guard around the global NUM_BASE) and `duration_kat` (call_ui.rs).
- The Base page (SettingsPage::Dozenal, render.rs) explains the scaling, what one is, and has a length legend beside time and size.

**Why:** Nick's "where do we draw the line" question; the same word (Tera) meant four peers, sixteen bits or a third depending on scale.
**How to apply:** before formatting any new numeral, ask which of the three kinds it is; magnitudes go thru a dms_* helper, counts thru fmt_num/fmt_num64, and hex must never see a scaled or M:SS value. Related: [[project_dms_age]], [[project_languages]], [[feedback_numbers_binary_at_rest]].
