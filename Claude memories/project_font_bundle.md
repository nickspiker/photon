---
name: project_font_bundle
description: Photon's font stack is 100% bundled (no system fonts, by design); what's actually covered, and the glyph-fallback contract
metadata:
  type: project
---

Photon/fluor render text with ZERO host-font reads — deliberate, for determinism/security/reliability (Nick 2026-09-08 restated it as a hard requirement).
Enforced in fluor `src/text.rs`: `Database::new()` (empty, never `load_system_fonts()`), a custom `BundledFallback` naming ONLY bundled families, and a hardcoded "en-US" locale so even cosmic-text's han-unification order can't vary by `LANG`. Motivating bug: a heart rendered on desktop (system fallback) and tofu'd on Android (cosmic-text's `PlatformFallback` returns an EMPTY list on Android).

**The glyph-fallback contract (MEASURED, not assumed — tests/glyph_fallback_probe.rs):** dozenal digits live at 0x10..0x1B in `Oxanium-Regular+glyphs.ttf`, and they resolve from ANY primary family. cosmic-text tries the primary face first, so a font carrying its own matching glyphs wins; only a miss falls through. Oxanium is now named FIRST in `common_fallback()` to make that deliberate (it previously worked only via cosmic-text's last-resort scan of every loaded face). **No per-font glyph art is needed, and PUA codepoints are unnecessary.** The long-standing "fmt_num would tofu in the bubble font" belief was false.

**Actual coverage (measured from cmaps):** Open Sans (Latin/Greek/Cyrillic) + Oxanium + Josefin Slab + Noto Sans Symbols 2 (2641 cps: hearts/stars/weather/dice/geometric) + Noto Color Emoji (10.8MB, feature `emoji`, enabled on all photon targets as of 1439865 — it was OFF before, so every emoji tofu'd).
**NOT covered, tofu today:** Arabic, Devanagari, Thai, CJK, Hangul, Armenian, Georgian, Runic; and — despite an old comment claiming otherwise — arrows (U+2190-21FF), box drawing (U+2500-257F), math operators (U+2200-22FF), ☮/♻. Those need Noto Sans Symbols (the FIRST one) + Noto Sans Math. Adding any font = add the .ttf, `load_font_data` it, name it in `common_fallback()`, and update the tofu-guard KAT.
