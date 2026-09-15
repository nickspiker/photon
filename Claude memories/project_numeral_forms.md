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

## Reputation, and why the scaling exists (2026-09-15)

Nick's frame: **"Reputation != Score. Arabic keeps score. USD keeps score."** Linear notation is the ledger's notation — it rewards accumulation and makes "how much more than me" legible. DMS resists both by construction, which is WHY dozenal-DMS is the default and arabic is the opt-out. His argument for DMS-as-the-only-count: "a billion dollars and a thousand move, do I notice? a million users to a million and ten, vs ten to twenty?" — perceived change is always relative. (Backed by Weber-Fechner + the log-number-line studies: linear counting is the SCHOOLED artifact, log is native.)

**The boundary I raised and he accepted:** subitizing. Below ~4 humans enumerate exactly rather than estimate — a different faculty, not an exception. So the line is PERCEPTION vs ENUMERATION: "can the user point at each one?" Yours-and-enumerable (devices, contacts, unread) = linear count; the world's-and-bulk (swarm peers, bytes, seconds) = magnitude. NOTE he then sharpened it further: with FRACTIONAL DMS the low end isn't lossy at all (3 = log2 1.58 = Zila·Luna vs 2 = bare Zila), so the word-floor idea is mostly unnecessary — only ZERO still needs a word, having no logarithm.

**PEERS should become a magnitude** (his catch): 20 peers = log2 4.32 = Tera. And `DmsScaleProse` currently teaches the count form with "Tera peers is four" — the one quantity that breaks the rule. Swap its illustration to something enumerable ("Tera devices is four"). Zero peers needs a word ("alone"). NOT YET BUILT.

**THE MATH (shipped as Base-page prose 2026-09-15, all 16 languages):**
- A grade is a BOUNDED share: radix point + ≤2 dozenal digits. Nobody can be a thousand times anybody.
- What fills is the SHORTFALL, not the grade: `1 − G = 1/E`, so **G = 1 − 1/E** (the "reciprocal" Nick intuited, falling out of "each doubling halves what's left").
- Two dozenal fraction digits = 144 levels, and `1 − 1/144 = 143/144 = .ƐƐ` EXACTLY. So the evidence denominator IS the display: a dozen behind you shows one digit, a gross shows two. **Displayed precision = support**, and an unearned digit is visibly false.
- Every rung is a unit fraction and 12 divides by 2/3/4/6, so all land exactly in dozenal and REPEAT FOREVER in decimal — base ten literally cannot write down a reputation. That table IS the dozenal argument, not an illustration of it.
- Quality rides too or volume buys the top: `G = q · (1 − 1/E)`, E weighted by DISTINCT people × duration (the two inputs volume cannot fake).
- **Never one scalar.** Grades are per-domain and don't add — averaging protocol marks with cooking marks is a lie about both. Two axes (grade = how well, doublings = how much to trust it) kept visibly separate, because any single number becomes a score once people sort by it.
- Helper: `crate::rep_grade_glyphs(evidence)` renders the ladder live (lib.rs).

**HELD, deliberately not in the prose** (Nick: "don't write that in but do mentally note"): this leads to voting, employment, insurance credibility — DOMAIN-WEIGHTED authority, heavy on a telecom protocol change, light on a food additive. Symbiosis/game theory. The prose sets it up without naming it via "it is about one thing, and that is the point, not a limitation" and "the average would be a lie about both halves".
