---
name: project_languages
description: "Translations-as-code: exhaustive-match Msg enum, 16 languages (2026-09-15); adding a string = a Msg variant every language file must answer; docs/languages.md"
metadata:
  type: project
---

Canonical: docs/languages.md. Approved 2026-09-03 ("Rust can already tell us if we missed a spot").
- src/ui/lang.rs = `enum Msg` (WHOLE messages with semantic params, never concatenated fragments) + `Lang` + `tr()` on a CURRENT atomic; src/ui/lang/<code>.rs = one exhaustive `text(Msg) -> Cow` match each. No runtime catalog or fallback — a partial language fails the build.
- **Adding a string:** the variant FIRST, then every language arm, then the call site. Widget constructor labels are frozen: re-set them per frame or add them to `relabel_for_language`. A multi-line passage is one '\n'-joined variant.
- Every numeral renders thru `crate::fmt_num` (decimal/dozenal aware) — [[project_numeral_forms]].
- Never translated: handles, voca words, log lines, dozenal digit names, VSF keys, "Photon"/"TOKEN"/"passless", WaveVoiceSentence (must match the English recording), picker autonyms.
- `display.lang` is fleet-linked (Nick: "if I speak Spanish I prolly want all my devices to"); seeded once from the OS locale thru `Lang::from_code` (the one seam), never live-follows the host. Picker = SettingsPage::Language, one autonym button per language.
- Autonym rule: what you'd say in "I'm a ___ speaker" (Tiếng Việt, Bahasa Indonesia, Kiswahili). `index()` = STORAGE order, `ALL` = DISPLAY order (sorted by autonym) — a picker slot resolves thru `Lang::ALL[slot]`.
- Terminology: voice = wave, video = beam, both transitive ("wave {party}"; es keeps the personal `a`; mi uses `i a`).
- Scripts: fluor shapes with full rustybuzz, so complex scripts render; Arabic's remaining cost is UI mirroring; CJK is a deferred payload decision; Thai needs dictionary line-breaking.
- Translating proofreads the English: a variant whose meaning or direction isn't in the English wants a doc comment naming its call site; carry numbers, never pre-formatted `&str`. Slavic 3-form plurals and Swahili noun classes need branches English lacks — the exhaustive match is what makes room for them.
- mi.rs is best-effort pending native review; Māori coinages are Nick's.

**Why:** the compiler is the completeness checker.
**How to apply:** follow the order above; read docs/languages.md before adding a language.
