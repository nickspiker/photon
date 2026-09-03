# Languages

Photon's UI is multilingual thru a compile-time catalog, not a runtime translation system.
There are no translation files to load, parse, or fall back from — the compiler is the completeness checker.

## The shape

- `src/ui/lang.rs` defines `enum Msg<'a>`: one variant per user-facing message, parameterized variants carry semantic values (`BoundDevice(&str)`, `PeersOnline(usize)`).
- Each language is one file (`lang/en.rs`, `lang/mi.rs`, `lang/es.rs`) exposing `fn text(Msg) -> Cow<'static, str>` as an **exhaustive match**.
- `lang::tr(msg)` dispatches on the language setting; call sites never hold raw literals.
- Adding a string = adding a `Msg` variant; every language file then fails to build until its arm exists.
No English can silently leak into a translated UI, no key can go missing at runtime.

## Why translations-as-code

- **Exhaustiveness is the feature.** A data catalog (gettext/Fluent/JSON) verifies completeness at runtime or via external tooling; the enum does it at `cargo check`.
- **Grammar is code.** Spanish gender agreement, Māori dual/plural and VSO word order — each language's arm is real Rust and branches however its grammar demands. No MessageFormat interpreter.
- **Whole messages, never fragments.** Variants are complete sentences with holes, so word order belongs to the translator. Concatenating translated fragments is structurally impossible.
- **Zero runtime cost.** A few hundred strings × a few languages compiles to nothing; nothing loads, nothing corrupts.

The cost: translators edit Rust.
That is deliberate — translations ride the same review as everything else, and a match arm of string literals is readable by anyone.
If non-programmer translators ever join, a build-script generating arms from a flat table is a mechanical add-on; the enum stays the source of truth.

## Rules

- Numbers inside messages render at the language edge per the number doctrine: dozenal glyphs via `dozenal_glyphs(n)`, arabic never.
- **Never translated**: handles (byte-precise, sacred), voca pairing words (protocol material), log lines (photonlog grep-ability dies the day logs localize), dozenal digit names (Zil/Ter/Lun/Stel are invented photon vocabulary, universal like the glyphs), VSF field names and storage keys.
- **Translated**: labels, hints, toasts, dialog prose, connection-ladder narration, weekday and month words.
- A new language starts as a copy of `en.rs` and translates incrementally — every arm exists from day one, content flips from English as it's done. Honest fallback with no fallback machinery.

## Selection

Language is a typed device-local setting (fstate), seeded once from the OS locale at first launch, then the user's — it never live-follows the host, so rendering stays deterministic.
The picker lives on the You/personal settings page; each language is named in itself (English, Español, Te Reo Māori).
Switching languages triggers a full damage/redraw; every layout measures text, so string-length changes just flow.

## Script ceiling

fluor NFC-normalizes and bundles Open Sans (precomposed Latin incl. āēīōū), Noto Symbols, optional Noto Color Emoji.
Any Latin-script language is a pure translation file.
CJK/RTL/Indic needs a fluor font bundle + bidi/shaping work first — a renderer project the catalog design doesn't care about.
The tell is structural: adding `ar.rs` blocks on fluor, not on strings.
