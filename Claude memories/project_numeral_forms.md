---
name: project_numeral_forms
description: "THE NUMBER FORMS (2026-09-11): counts linear, magnitudes DMS = floor(log2) spelled dozenal, proportions one dot-fraction digit; hex linear everywhere; reputation = bounded grade G = q·(1 − 1/E); docs/dozenal.md"
metadata:
  type: project
---

Canonical: docs/dozenal.md. Decided with Nick 2026-09-11/15.
- **Three forms by kind of number:** counts (enumerable — devices, contacts, unread, chunk 3 of 5) are linear in the base; magnitudes (size, age, rate, length, bulk the user can't point at) are DMS; proportions are one dozenal fraction digit with a dot (.Lun = half). Identifiers never convert. The line = perception vs enumeration ("can the user point at each one?").
- **DMS = doublings, spelled dozenal:** floor(log2 x) in base twelve (the +1 bit-length form was dropped: "should it really be Zil? Not Zila"); zero reads as a word (now / empty / alone).
- Units (the "1" of each scale) are anchored on the hydrogen line; the current model is [[project_eagle_units_first_principles]] (the 2026-09-11 Eagle-second age unit is superseded); size counts bits.
- **Hex is linear everywhere:** plain seconds, RTT in seconds with a hex fraction, sizes in bits, stops thru `fmt_halves`. Diagnostic timestamps stay arabic clock time. Tests: `base_kat`, `duration_kat`.
- The Base page (SettingsPage::Dozenal) teaches it.
**Reputation (2026-09-15; Nick: "Reputation != Score. Arabic keeps score. USD keeps score."):** a grade is a BOUNDED share — radix point + ≤2 dozenal digits; G = q · (1 − 1/E), E weighted by distinct people × duration; 143/144 = .ƐƐ exactly, so displayed precision = support. Never one scalar: grades are per-domain, and the two axes (grade vs doublings of trust) stay visibly separate. HELD, not in prose (Nick: "don't write that in but do mentally note"): it leads to domain-weighted authority (voting, employment, insurance).

**Why:** the same word meant four peers, sixteen bits or a third depending on scale.
**How to apply:** before formatting any numeral, ask which kind it is; magnitudes go thru a dms_* helper, counts thru fmt_num/fmt_num64; hex never sees a scaled or M:SS value. Related: [[project_dms_age]], [[project_languages]].
