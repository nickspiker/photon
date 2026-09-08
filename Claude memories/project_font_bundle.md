---
name: project_font_bundle
description: Photon's font stack is 100% bundled (no system fonts, by design); what's actually covered, and the glyph-fallback contract
metadata:
  type: project
---

Photon/fluor render text with ZERO host-font reads — deliberate, for determinism/security/reliability (Nick 2026-09-08 restated it as a hard requirement).
Enforced in fluor `src/text.rs`: `Database::new()` (empty, never `load_system_fonts()`), a custom `BundledFallback` naming ONLY bundled families, and a hardcoded "en-US" locale so even cosmic-text's han-unification order can't vary by `LANG`. Motivating bug: a heart rendered on desktop (system fallback) and tofu'd on Android (cosmic-text's `PlatformFallback` returns an EMPTY list on Android).

**The glyph-fallback contract (MEASURED, not assumed — tests/glyph_fallback_probe.rs):** dozenal digits live at 0x10..0x1B in `Oxanium-Regular+glyphs.ttf`, and they resolve from ANY primary family. cosmic-text tries the primary face first, so a font carrying its own matching glyphs wins; only a miss falls through. Oxanium is now named FIRST in `common_fallback()` to make that deliberate (it previously worked only via cosmic-text's last-resort scan of every loaded face). **No per-font glyph art is needed, and PUA codepoints are unnecessary.** The long-standing "fmt_num would tofu in the bubble font" belief was false.

**Coverage as of 2026-09-08 (measured from cmaps, loader = src/ui/fonts.rs::load_bundled, KAT = tests/glyph_fallback_probe.rs):** Open Sans + Oxanium(+glyphs) + Josefin Slab + Noto Sans Symbols 2 + Noto Color Emoji (feature on all targets) + Noto Sans Symbols / Math / Mono (mono symbols, arrows, math, box drawing, currency) + Noto Sans Arabic / Devanagari / Thai / Armenian / Georgian / Runic (~2.4MB total, assets/Noto, OFL).
**NOT bundled, tofu by decision:** CJK + Hangul (10–20MB; Nick: "skip it for now… maybe roll that in later").
**Colour rule (Nick 2026-09-08):** Noto Color Emoji sits BEFORE the mono symbol faces in fluor's chain, so colour wins whenever a colour glyph exists. Override per character with variation selectors — U+FE0F forces colour, U+FE0E forces mono — implemented in fluor `set_text_vs` as span-level family routing (cosmic-text can't do it in-font: fallback fires per missing glyph and ignores selectors). UI chrome pins ▶ with FE0E so Play matches ■ Stop; ☎ ⚠ 📹 🔊 📎 render colour.
Adding a font = drop the .ttf in assets/Noto, load it in fonts.rs, name it in fluor's chain (common_fallback or script_fallback), extend the KAT.

**Trimming (2026-09-08, Nick: "cut the cruft and ship the fonts and don't condense to one"):** NO Python in the tree (scripts/lib/comment-gate.sh NO-PYTHON GATE) — fontTools/pyftsubset is forbidden; the tool is `tools/font-trim.rs`, a std-only rustc bitty-executable (same convention as tools/arch-gate.rs), driven by `scripts/fonts/build.sh`. It cuts a STATIC TrueType font to owned Unicode blocks with glyph ids stable (cmap + glyf/loca rewritten, nothing else), or `--compact` (dense renumber) which it REFUSES unless every gid-bearing layout table was `--drop`ped. Mono 582→30 KB, Math 967→358 KB; sources committed under assets/Noto/sources. Variable script fonts ship whole (their bulk is shaping tables). The KAT `trimmed_statics_match_their_sources` asserts bit-exact advance parity with the untrimmed sources — it caught the compact-cmap-old-ids bug (glyphs out of range lay out as NOTHING, which the "≠ tofu" check had passed vacuously; coverage tests now also assert w > 0).
