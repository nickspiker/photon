---
name: project_font_bundle
description: "Photon's font stack is 100% bundled (no system fonts, by design): coverage, the glyph-fallback contract, FE0F/FE0E colour routing, Rust-only font trimming"
metadata:
  type: project
---

Photon/fluor read ZERO host fonts (Nick 2026-09-08, hard: determinism, security, reliability).
Enforced in fluor `src/text.rs`: empty `Database::new()` (never `load_system_fonts()`), `BundledFallback` naming only bundled families, hardcoded "en-US" locale.
Motivating bug: a heart rendered on desktop via system fallback and tofu'd on Android (cosmic-text's PlatformFallback is empty there).

**Fallback contract (measured, tests/glyph_fallback_probe.rs):** cosmic-text tries the primary face first, and a miss falls thru the chain.
Dozenal digits live at 0x10..0x1B in `Oxanium-Regular+glyphs.ttf`; Oxanium is FIRST in `common_fallback()`, so they resolve from any primary family — no PUA, no per-font glyph art.
**Bundled** (src/ui/fonts.rs::load_bundled, assets/Noto, OFL): Open Sans, Oxanium(+glyphs), Josefin Slab, Noto Color Emoji, Noto Sans Symbols / Symbols 2 / Math / Mono, Arabic / Devanagari / Thai / Armenian / Georgian / Runic.
**Not bundled:** CJK + Hangul (10–20 MB; Nick's call to revisit).
**Colour rule:** Noto Color Emoji sits before the mono symbol faces; U+FE0F forces colour, U+FE0E forces mono, via fluor `set_text_vs` span-level family routing. UI chrome pins ▶ with FE0E so Play matches ■ Stop.
**Trimming:** no Python ([[feedback_no_python]]); `tools/font-trim.rs` (std-only rustc tool) via `scripts/fonts/build.sh` cuts static fonts to owned blocks with stable glyph ids (`--compact` refused unless every gid-bearing table is dropped); sources in assets/Noto/sources.
KAT `trimmed_statics_match_their_sources` asserts advance parity; coverage tests assert width > 0 (an out-of-range glyph lays out as nothing, not tofu).

**Why:** a glyph must render identically on every platform, and host fonts are an outside input.
**How to apply:** adding a font = .ttf in assets/Noto, load it in fonts.rs, name it in fluor's chain (common_fallback or script_fallback), extend the KAT.
