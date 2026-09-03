---
name: project_languages
description: "language catalog SHIPPED 2026-09-03 — translations-as-code (exhaustive-match enum), en+es+mi complete, picker on You page; adding a string = add a Msg variant, every language file fails to build until its arm exists"
metadata: 
  node_type: memory
  type: project
  originSessionId: 8c6221f3-48a5-4f77-b00e-1e3c7fa4c3b5
---

Nick approved translations-as-code 2026-09-03 ("Rust can already tell us if we missed a spot. Build it!"). Full arc shipped same night.

**The shape (docs/languages.md is canonical):**
- src/ui/lang.rs = enum Msg (~330 variants, WHOLE messages with semantic params — never concatenated fragments) + Lang{En,Es,Mi} + tr() dispatch on a CURRENT atomic (same shape as DOZENAL_UI).
- lang/en.rs, lang/es.rs, lang/mi.rs — one exhaustive `text(Msg) -> Cow` match each; the compiler is the completeness checker, no runtime catalog/fallback machinery. A new language starts as a copy of en.rs and translates incrementally.
- Never translated: handles, voca words, log lines (photonlog grep-ability), dozenal digit names, VSF keys, "Photon"/"TOKEN"/"passless" heads, WaveVoiceSentence (must match the ENGLISH audio recording — translate only with a localized recording), picker autonyms (English/Español/Te Reo Māori — a lost user must recognise their own tongue).

**Number mandate folded in (Nick same night: "make sure every number gets formatted either decimal or dozenal"):** EVERY numeral in en/es/mi renders thru crate::fmt_num (DOZENAL_UI-aware), including numerals that sat frozen in prose (25 MB, 24–48h, ~15s, Step 1/2, Webster's 1960) and the "(s)" plural hacks → real plural branches. Exceptions kept decimal with comments: FileBubble sizes (bubble font can't render glyph control bytes — Oxanium-only), fmt_duration's deliberate decimal branch. Glyph-face caveat: fmt_num output needs Oxanium at the draw site.

**Selection:** device-local typed `display.lang` (x-string code, set_link(false) like display.zoom), seeded ONCE from OS locale (platform/locale.rs os_language: LC_* ladder / GetUserDefaultLocaleName; Android seeds en until a JNI sniff), then the user's — never live-follows the host. Picker = YouRow::Language dropdown on the You page; change polled via take_change in advance_protocol → set_lang + save + relabel_for_language (constructor-labeled widgets: theme-dropdown options via fluor Dropdown::set_options @10d9f63, all settings checkboxes; per-frame set_label sites self-heal next paint).

**Traps for future string work:** add the variant FIRST, then all three language arms, then the call site (compile errors guide the order). Widget constructor labels are frozen — either re-set per frame in render or add to relabel_for_language. Multi-line passages are ONE '\n'-joined variant; call sites iterate .lines() (PermanenceWarning gives line 0 the headline colour). es judgment calls logged in the 2026-09-03 session (darse de baja, Echar, Puente…); mi is my best-effort pending native review — flagged in mi.rs header.

**Script ceiling:** any Latin-script language is a pure translation file; CJK/RTL blocks on fluor (font bundle + bidi), not on strings.
