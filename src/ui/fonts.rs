//! Every font photon draws with, loaded from the binary — the whole set, in one place, so the app and the KAT that guards the contract load the SAME bundle.
//!
//! No host font is ever read (fluor builds its font db empty and its fallback chain names only bundled families): the same bytes render the same glyph on every platform, or tofu on every platform. Coverage is what this file loads, nothing more — see fluor's `BundledFallback` for the chain order and the per-script routing, and tests/glyph_fallback_probe.rs for the measured contract.

/// Load photon's bundled faces into fluor's shared text renderer. Idempotent in effect (fontdb keeps duplicate faces harmlessly), cheap (byte slices already in the binary).
pub fn load_bundled(text: &mut fluor::text::TextRenderer) {
    let db = text.font_system_mut().db_mut();
    // Oxanium, VARIABLE (wght 200..800) WITH the dozenal digits (Nick 2026-10-14: "copy the dozenal over and get it all variable please"): one face draws every weight the wordmark (800), the pills and the numerals ask for, where seven static cuts used to. The dozenal digits live in the reserved control block 0x10-0x1b (DLE..ESC = digits 0..11, Zil..Stelor); fluor names Oxanium first in its fallback chain, so fmt_num output finds them from any primary family. They were drawn at Regular; their other weights were synthesized once, in proportion to Oxanium's own stems (each point moved along its miter by the 'I' stem's change at 200 and 800, scaled to the digits' heavier stroke; counters grow inward at half rate so the loops stay open at 800), from Oxanium-VariableFont_wght.ttf + the static Oxanium-Regular+glyphs.ttf. The FONT is the artifact, committed here — no generator lives in this repo (no Python, Nick 2026-10-14); the one-off script is in git history at 588dfb01 should the digits ever be redrawn.
    db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-VariableFont_wght+glyphs.ttf").to_vec());
    // Monochrome symbol coverage beyond fluor's Symbols 2 (Nick 2026-09-08, measured from the cmaps — the package blurbs lie): Symbols carries ☮ ☯ ♻ and the misc/dingbat ranges; Math carries the arrows (U+2190-21FF) and operators (U+2200-22FF) in full; Mono is the only one with all of box drawing (U+2500-257F) and the currency block (U+20A0-20CF).
    // Mono and Math are CUT to exactly those blocks by scripts/fonts/build.sh (tools/font-trim.rs, sources under assets/Noto/sources): no Latin alphabet Open Sans already draws, no kerning/shaping tables a symbol never needs, minus everything Symbols 2 owns — so every codepoint has one monochrome home. 582+967 KB → 30+358 KB. The variable script fonts ship whole: their bulk is shaping, not spare Latin.
    db.load_font_data(include_bytes!("../../assets/Noto/NotoSansSymbols[wght].ttf").to_vec());
    db.load_font_data(include_bytes!("../../assets/Noto/NotoSansMath-Regular.ttf").to_vec());
    db.load_font_data(include_bytes!("../../assets/Noto/NotoSansMono-Regular.ttf").to_vec());
    // Scripts, routed per-script by fluor. CJK deliberately absent for now (10-20 MB; Nick's call to revisit).
    db.load_font_data(include_bytes!("../../assets/Noto/NotoSansArabic[wght].ttf").to_vec());
    db.load_font_data(include_bytes!("../../assets/Noto/NotoSansDevanagari[wght].ttf").to_vec());
    db.load_font_data(include_bytes!("../../assets/Noto/NotoSansThai[wght].ttf").to_vec());
    db.load_font_data(include_bytes!("../../assets/Noto/NotoSansArmenian[wght].ttf").to_vec());
    db.load_font_data(include_bytes!("../../assets/Noto/NotoSansGeorgian[wght].ttf").to_vec());
    db.load_font_data(include_bytes!("../../assets/Noto/NotoSansRunic-Regular.ttf").to_vec());
}
