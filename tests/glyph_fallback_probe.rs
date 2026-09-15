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
        assert!(w > 0.0, "{name} ({s:?}) laid out as nothing — a glyph id out of range reads as resolved-but-empty, which is worse than tofu");
        assert_ne!(w, tofu, "{name} ({s:?}) measured as tofu: not resolved by the bundle");
    }
}

/// The LANGUAGE-CATALOG contract (docs/languages.md "Script ceiling"): every letter the shipped translations actually type must resolve from Open Sans itself, with no script routing and no host font.
/// This is what makes those languages pure translation files, and the doc now claims it in writing — so it is measured here rather than believed. Vietnamese is the one that could plausibly fail: its letters live in Latin Extended Additional, not the basic block, and it stacks two marks on one vowel.
/// Cyrillic is included because it is the reason Russian and its neighbours need no font work at all.
#[test]
fn shipped_language_letters_resolve_from_open_sans() {
    let mut tr = renderer(true);
    let tofu = width(&mut tr, "\u{6F22}", "Open Sans");
    let letters = [
        ("Portuguese", "\u{E3}\u{F5}\u{E7}"),              // ã õ ç
        ("French", "\u{E9}\u{E8}\u{EA}\u{FB}\u{E7}"),      // é è ê û ç
        ("German", "\u{E4}\u{F6}\u{FC}\u{DF}"),            // ä ö ü ß
        ("Turkish", "\u{131}\u{130}\u{11F}\u{15F}"),       // ı İ ğ ş — dotted/dotless i are DISTINCT letters
        ("Vietnamese", "\u{1EBF}\u{1ED9}\u{1EEF}\u{111}\u{1A1}\u{1B0}"), // ế ộ ữ đ ơ ư — precomposed, two marks on one vowel
        ("Maori", "\u{101}\u{113}\u{12B}\u{14D}\u{16B}"),  // ā ē ī ō ū
        ("Cyrillic", "\u{410}\u{44F}\u{452}\u{491}"),      // А я ђ ґ — Russian plus the Serbian/Ukrainian extras
        ("Greek", "\u{3B1}\u{3A9}"),                        // α Ω
        ("Polish/Czech", "\u{142}\u{119}\u{159}\u{17E}"),  // ł ę ř ž
    ];
    for (name, s) in letters {
        for ch in s.chars() {
            let w = width(&mut tr, &ch.to_string(), "Open Sans");
            assert!(w > 0.0, "{name}: U+{:04X} laid out as nothing — resolved-but-empty is worse than tofu", ch as u32);
            assert_ne!(w, tofu, "{name}: U+{:04X} measured as tofu — Open Sans does not cover it, so that language is NOT a pure translation file", ch as u32);
        }
    }
    // Indonesian and Swahili are plain ASCII Latin — no probe needed, and that is precisely why they are the cheapest reach on the board.
}

/// COMPLEX SHAPING actually runs, which is the load-bearing claim behind docs/languages.md's roadmap: Devanagari needs only a translation pass because the shaper — not the translator, and not a layout project — composes its conjuncts, and Arabic's remaining cost is UI mirroring rather than text.
/// Measured structurally, so it cannot pass by accident: a shaped form must be NARROWER than its unshaped parts, which is only true if rustybuzz substituted glyphs rather than laying codepoints out one by one.
/// If fluor ever drops to `Shaping::Basic`, or a script loses its fallback route, both assertions fail here rather than on someone's phone in a language nobody on the team reads.
#[test]
fn complex_scripts_actually_shape() {
    let mut tr = renderer(true);
    // Devanagari conjunct: क + virama + ष composes the single ligature क्ष. Unshaped it would be three separate glyphs INCLUDING a visible virama, so it cannot be narrower than the two bare consonants.
    let ka = width(&mut tr, "\u{915}", "Open Sans");
    let ssa = width(&mut tr, "\u{937}", "Open Sans");
    let conjunct = width(&mut tr, "\u{915}\u{94D}\u{937}", "Open Sans");
    assert!(conjunct > 0.0, "the conjunct laid out as nothing");
    assert!(conjunct < ka + ssa, "Devanagari conjunct did not compose: क्ष measured {conjunct}, wider than bare क + ष at {}", ka + ssa);
    // Arabic contextual forms: two behs joined take initial+final forms, narrower than two isolated ones.
    let beh = width(&mut tr, "\u{628}", "Open Sans");
    let joined = width(&mut tr, "\u{628}\u{628}", "Open Sans");
    assert!(joined > 0.0, "the Arabic pair laid out as nothing");
    assert!(joined < beh * 2.0, "Arabic did not apply contextual forms: بب measured {joined}, not narrower than two isolated ب at {}", beh * 2.0);
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

/// The trimmed static fonts (tools/font-trim, --compact) must render EXACTLY like their untrimmed sources: same advances for box drawing, currency, arrows and operators. A wrong hmtx, a mis-renumbered composite, or a broken cmap segment all show up here as a width delta.
#[test]
fn trimmed_statics_match_their_sources() {
    let mut trimmed = renderer(true);
    let mut source = TextRenderer::new();
    {
        let db = source.font_system_mut().db_mut();
        db.load_font_data(std::fs::read("assets/Noto/sources/NotoSansMono-Regular.ttf").expect("mono source"));
        db.load_font_data(std::fs::read("assets/Noto/sources/NotoSansMath-Regular.ttf").expect("math source"));
    }
    let probes = [
        ("box", "\u{250C}\u{2500}\u{2510}\u{2554}\u{2550}\u{2557}\u{2591}", "Noto Sans Mono"),
        ("currency", "\u{20B9}\u{20A9}\u{20B4}\u{20A6}\u{20AC}", "Noto Sans Mono"),
        ("arrows", "\u{2190}\u{2192}\u{21D4}\u{27F3}\u{2B06}", "Noto Sans Math"),
        ("operators", "\u{2200}\u{2203}\u{2205}\u{2207}\u{2208}\u{2261}\u{2282}\u{2295}\u{2297}", "Noto Sans Math"),
        ("alphanumerics", "\u{1D400}\u{1D49C}\u{1D538}", "Noto Sans Math"),
    ];
    for (name, s, family) in probes {
        let a = width(&mut trimmed, s, family);
        let b = width(&mut source, s, family);
        assert!(a > 0.0, "{name}: trimmed font produced nothing");
        assert_eq!(a, b, "{name} ({family}): trimmed {a} vs source {b}");
    }
}
