//! The bundled-font contract, measured rather than assumed.
//!
//! Everything here loads the SAME bundle the app ships (photon_messenger::ui::fonts::load_bundled) and pins facts that were previously believed on faith — one of which was wrong for months:
//!   1. A dozenal glyph byte (0x10..0x1B) resolves to the Oxanium `+glyphs` face from ANY primary family. No per-font glyph art.
//!   2. Every bundled script and symbol range actually resolves; what we do NOT bundle (CJK, Hangul) measures as tofu — identically on every platform, because no host font is ever consulted. If the tofu case ever starts resolving, a system-font path has crept in.
//!   3. Variation selectors choose the face: U+FE0F forces colour emoji, U+FE0E forces the monochrome symbol face, and the two measure differently.
use fluor::text::{TextRenderer, TextStyle};

fn renderer(bundle: bool) -> TextRenderer {
    let mut tr = TextRenderer::new();
    if bundle {
        photon_messenger::ui::fonts::load_bundled(&mut tr);
    }
    tr
}

fn width(tr: &mut TextRenderer, s: &str, family: &'static str) -> f32 {
    tr.measure_text(s, &TextStyle::new(32.0, 0).font(family))
}

/// Loading the bundle must CHANGE how an Open Sans run measures the dozenal block, to exactly Oxanium's own metrics. Without it the block measures like a codepoint nothing covers.
#[test]
fn dozenal_block_falls_back_to_oxanium_from_any_primary() {
    let glyphs = "\u{10}\u{1B}";
    let mut with = renderer(true);
    let mut without = renderer(false);
    let w_with = width(&mut with, glyphs, "Open Sans");
    let w_oxanium = width(&mut with, glyphs, "Oxanium");
    let w_without = width(&mut without, glyphs, "Open Sans");
    let tofu = width(&mut without, "\u{6F22}\u{6F22}", "Open Sans");
    assert!((w_with - w_oxanium).abs() < 0.01, "Open Sans run must adopt Oxanium's dozenal metrics ({w_with} vs {w_oxanium})");
    assert!((w_without - tofu).abs() < 0.01, "with no glyph face the block must measure as tofu ({w_without} vs {tofu})");
}

/// Every bundled SCRIPT routes to its own face (an Open Sans run measures exactly what the script's family measures directly — that is fluor's per-script fallback doing its job), every bundled symbol range resolves, and CJK/Hangul — which we don't bundle — measure exactly like tofu.
/// Exact comparisons on purpose: a notdef advance is bit-identical every time, and a real glyph that happens to be nearly as wide (Thai ก is 19.2 to tofu's 19.203125) must still count as resolved.
#[test]
fn bundled_coverage_resolves_and_unbundled_is_tofu() {
    let mut tr = renderer(true);
    let tofu = width(&mut tr, "\u{6F22}", "Open Sans");
    let hangul = width(&mut tr, "\u{D55C}", "Open Sans");
    assert_eq!(hangul, tofu, "Hangul must be tofu like CJK — neither is bundled");
    let scripts = [
        ("Arabic", "\u{0627}\u{0628}", "Noto Sans Arabic"),
        ("Devanagari", "\u{0905}\u{0915}", "Noto Sans Devanagari"),
        ("Thai", "\u{0E01}\u{0E02}", "Noto Sans Thai"),
        ("Armenian", "\u{0531}\u{0532}", "Noto Sans Armenian"),
        ("Georgian", "\u{10D0}\u{10D1}", "Noto Sans Georgian"),
        ("Runic", "\u{16A0}\u{16A2}", "Noto Sans Runic"),
    ];
    for (name, s, family) in scripts {
        let routed = width(&mut tr, s, "Open Sans");
        let direct = width(&mut tr, s, family);
        assert_eq!(routed, direct, "{name} must route to {family} ({routed} vs {direct})");
        assert_ne!(routed, tofu * 2.0, "{name} resolved to tofu");
    }
    let symbols = [
        ("arrow", "\u{2192}"), ("double arrow", "\u{21D4}"), ("math", "\u{2200}"), ("box single", "\u{250C}"), ("box double", "\u{2554}"),
        ("rupee", "\u{20B9}"), ("won", "\u{20A9}"), ("hryvnia", "\u{20B4}"), ("naira", "\u{20A6}"),
        ("peace", "\u{262E}"), ("recycle", "\u{267B}"), ("emoji", "\u{1F600}"), ("emoji 2020", "\u{1FAE7}"),
    ];
    for (name, s) in symbols {
        let w = width(&mut tr, s, "Open Sans");
        assert_ne!(w, tofu, "{name} ({s:?}) measured as tofu: not resolved by the bundle");
    }
}

/// U+FE0F and U+FE0E pick the face for the character they follow: the colour-emoji ☎ and the monochrome ☎ are different glyphs with different advances, and a bare ☎ equals the colour one (colour-first chain).
#[test]
fn variation_selectors_choose_the_face() {
    let mut tr = renderer(true);
    let colour = width(&mut tr, "\u{260E}\u{FE0F}", "Open Sans");
    let mono = width(&mut tr, "\u{260E}\u{FE0E}", "Open Sans");
    let bare = width(&mut tr, "\u{260E}", "Open Sans");
    let mono_direct = width(&mut tr, "\u{260E}", "Noto Sans Symbols 2");
    let colour_direct = width(&mut tr, "\u{260E}", "Noto Color Emoji");
    assert!((colour - mono).abs() > 0.01, "FE0F and FE0E must land on different faces (both {colour})");
    assert!((mono - mono_direct).abs() < 0.01, "FE0E must route to the mono symbol face ({mono} vs {mono_direct})");
    assert!((colour - colour_direct).abs() < 0.01, "FE0F must route to the colour emoji face ({colour} vs {colour_direct})");
    assert!((bare - colour_direct).abs() < 0.01, "a bare symbol follows the colour-first chain ({bare} vs {colour_direct})");
}
