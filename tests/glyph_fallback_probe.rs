//! The bundled-font contract, measured rather than assumed.
//!
//! Two properties this KAT nails down, both of which were previously believed on faith and one of which was WRONG:
//!   1. A dozenal glyph byte (0x10..0x1B) resolves to the Oxanium `+glyphs` face even when the primary family is Open Sans — so numerals render correctly in message bubbles, file sizes, anywhere. No per-font glyph art required.
//!   2. Nothing renders via a HOST font. Coverage is exactly what we bundle; a codepoint we don't ship is tofu on every platform identically.
use fluor::text::{TextRenderer, TextStyle};

fn width(with_oxanium: bool, s: &str, family: &'static str) -> f32 {
    let mut tr = TextRenderer::new();
    if with_oxanium {
        tr.font_system_mut().db_mut().load_font_data(
            std::fs::read("assets/Oxanium/Oxanium-Regular+glyphs.ttf").expect("oxanium asset"),
        );
    }
    tr.measure_text(s, &TextStyle::new(32.0, 0).font(family))
}

/// Loading Oxanium must CHANGE how an Open Sans run measures the dozenal block, and change it to exactly Oxanium's own metrics. Without it the block measures identically to a codepoint we bundle no glyph for at all — which is what tofu costs.
#[test]
fn dozenal_block_falls_back_to_oxanium_from_any_primary() {
    let glyphs = "\u{10}\u{1B}";
    let with = width(true, glyphs, "Open Sans");
    let without = width(false, glyphs, "Open Sans");
    let oxanium = width(true, glyphs, "Oxanium");
    let tofu = width(true, "\u{2500}\u{2500}", "Open Sans");
    assert!((with - oxanium).abs() < 0.01, "Open Sans run must adopt Oxanium's dozenal metrics ({with} vs {oxanium})");
    assert!((without - tofu).abs() < 0.01, "with no glyph face loaded the block must measure as tofu ({without} vs {tofu})");
    assert!((with - without).abs() > 1.0, "loading the glyph face must visibly change the measurement");
}

/// Determinism guard: coverage comes from the bundle, never the host. If this ever starts passing for an unbundled script, a system-font path has crept in — see BundledFallback in fluor.
#[test]
fn unbundled_scripts_are_tofu_not_host_fonts() {
    // Arabic, Devanagari, Thai, CJK, Armenian, Georgian, Runic, arrows, box drawing, math — none bundled today.
    for s in ["\u{0627}", "\u{0905}", "\u{0E01}", "\u{6F22}", "\u{0531}", "\u{10D0}", "\u{16A0}", "\u{2192}", "\u{250C}", "\u{2200}"] {
        let w = width(true, s, "Open Sans");
        let tofu = width(true, "\u{2500}", "Open Sans");
        assert!((w - tofu).abs() < 0.01, "{s:?} measured {w}, expected tofu {tofu} — a host font may have leaked into the fallback chain");
    }
}
