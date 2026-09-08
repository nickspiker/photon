//! Every font photon draws with, loaded from the binary — the whole set, in one place, so the app and the KAT that guards the contract load the SAME bundle.
//!
//! No host font is ever read (fluor builds its font db empty and its fallback chain names only bundled families): the same bytes render the same glyph on every platform, or tofu on every platform. Coverage is what this file loads, nothing more — see fluor's `BundledFallback` for the chain order and the per-script routing, and tests/glyph_fallback_probe.rs for the measured contract.

/// Load photon's bundled faces into fluor's shared text renderer. Idempotent in effect (fontdb keeps duplicate faces harmlessly), cheap (byte slices already in the binary).
pub fn load_bundled(text: &mut fluor::text::TextRenderer) {
    let db = text.font_system_mut().db_mut();
    // Oxanium weights for the wordmark and pills: ExtraLight/Light/Regular/Medium/SemiBold/Bold/ExtraBold = 200/300/400/500/600/700/800. The logo uses 800.
    db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-ExtraLight.ttf").to_vec());
    db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-Light.ttf").to_vec());
    // Regular is the `+glyphs` superset: plain Oxanium-Regular for 0x20-0x7e plus the dozenal digits in the reserved control block 0x10-0x1b (DLE..ESC = digits 0..11, Zil..Stelor). fluor names Oxanium first in its fallback chain, so a dozenal numeral resolves from ANY primary family — no draw site opts in.
    db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-Regular+glyphs.ttf").to_vec());
    db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-Medium.ttf").to_vec());
    db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-SemiBold.ttf").to_vec());
    db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-Bold.ttf").to_vec());
    db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-ExtraBold.ttf").to_vec());
    // Monochrome symbol coverage beyond fluor's Symbols 2 (Nick 2026-09-08, measured from the cmaps — the package blurbs lie): Symbols carries ☮ ☯ ♻ and the misc/dingbat ranges; Math carries the arrows (U+2190-21FF) and operators (U+2200-22FF) in full; Mono is the only one with all of box drawing (U+2500-257F) and the currency block (U+20A0-20CF).
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
