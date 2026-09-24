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

- Numbers inside messages render at the language edge per the number doctrine: dozenal glyphs via `dozenal_glyphs(n)`, arabic never. The three forms (counts, doublings, fractions), the scaling and each scale's unit are in docs/dozenal.md.
- **Never translated**: handles (byte-precise, sacred), voca pairing words (protocol material), log lines (photonlog grep-ability dies the day logs localize), dozenal digit names (Zil/Ter/Lun/Stel are invented photon vocabulary, universal like the glyphs), VSF field names and storage keys.
- **Translated**: labels, hints, toasts, dialog prose, connection-ladder narration, weekday and month words.
- A new language starts as a copy of `en.rs` and translates incrementally — every arm exists from day one, content flips from English as it's done. Honest fallback with no fallback machinery.

## Selection

Language is a typed device-local setting (fstate), seeded once from the OS locale at first launch, then the user's — it never live-follows the host, so rendering stays deterministic.
The picker lives on the You/personal settings page; each language is named in itself, bare and parallel: English, Español, Māori — not "Te Reo Māori", since te reo just means "the language" (Nick 2026-09-03).
`Lang::index` is the stable storage/atomic order and never moves once shipped; `Lang::ALL` is display order and may be re-sorted freely.
Switching languages triggers a full damage/redraw; every layout measures text, so string-length changes just flow.

## Script ceiling — what the renderer can actually do

Measured 2026-09-15, because this section previously claimed a ceiling the code had already broken through.

- **Shaping is ON.** fluor drives cosmic-text with `Shaping::Advanced` at every call site, which is full rustybuzz shaping: Arabic contextual forms and ligatures, Devanagari conjuncts and matra reordering, all of it. There is no shaping engine left to write.
- **Per-script routing exists.** fluor's `BundledFallback::script_fallback` already maps Arabic, Devanagari, Thai, Armenian, Georgian and Runic to their bundled faces.
- **The faces are bundled.** `src/ui/fonts.rs` loads Noto Sans Arabic, Devanagari, Thai, Armenian and Georgian today.
- **Open Sans already covers more than Latin.** Measured against the bundled `OpenSans-Regular.ttf` (1010 codepoints): Vietnamese precomposed (ế ộ ữ đ ơ ư), Turkish (ı İ ğ ş), Polish/Czech (ł ę ř ž), Cyrillic including extended (А я ђ ґ), and Greek all resolve. **Cyrillic needs no font work** — Russian, Ukrainian, Serbian, Bulgarian and Kazakh are pure translation files.
- **CJK is the one real gap**, and it is deliberate: no CJK face is bundled because it costs 10–20 MB, which collides with the no-host-fonts doctrine. That is a payload decision, not a rendering one.
- **What Arabic still needs is UI MIRRORING, not text.** Right-to-left *text* renders correctly today; a right-to-left *interface* — rail side, bubble sides, back-arrow direction, alignment defaults — is a photon/fluor layout project. That is the whole remaining cost, and it is smaller than "write a shaping engine" but it is not nothing.

Consequence: **Devanagari is left-to-right**, so Hindi needs no mirroring at all. Font, shaping and routing are already in place, which puts Hindi within reach of a pure translation pass plus a rendering verification against `tests/glyph_fallback_probe.rs`.

## Reach roadmap

Approximate total speakers (L1+L2). Ordered by reach per unit of engineering, not by headcount.

| Tier | Cost beyond ~354 strings | Languages |
|---|---|---|
| Pure translation | none | Portuguese 265M · French 310M · Indonesian 200M · German 135M · Swahili 200M · Vietnamese 85M · Turkish 90M · **Russian 255M** · Polish · Italian · Dutch |
| Translation + render check | verify the glyph probe | Hindi 610M · Thai 60M (faces + shaping already bundled) |
| Translation + UI mirroring | RTL layout project | Arabic 475M · Persian 80M · Urdu 230M |
| Translation + payload decision | 10–20 MB of font | Mandarin · Japanese 125M · Korean 80M |

**Raw speaker counts mislead for a sovereign messenger, in both directions.** Mandarin's 1.1B badly overstates addressable users, because the traversal ladder leans on FGTW (a Cloudflare worker) and Cloudflare is blocked in the PRC — the realistic reach is Taiwan, Singapore, Malaysia and the diaspora, and Traditional/Simplified splits even that in two. Meanwhile Arabic, Persian and Urdu *understate* the case: those are populations for whom messaging that touches no infrastructure is the point, not a feature. Effort-per-user is the wrong sole metric when the users hardest to reach are the ones with the most need.

**Recommended order:** the pure-translation tier wholesale (~1.5B reach, zero engine work), then Hindi (biggest single number that costs only a translation pass), then the RTL mirroring project with Arabic as its first tenant — the mirroring is a one-time investment that Persian and Urdu then inherit. CJK last, gated on the payload decision, and worth reopening only if the subset-versus-bundle question gets a good answer.

**Keep a values slot each round.** Māori is in the catalog ahead of languages a hundred times its size, and that was correct. If the ordering were purely by headcount it would say something about the project that the project does not mean.

## Translation quality

Security vocabulary is where machine translation fails quietly.
"Sealed", "fold", "standing member", "lane", "braid", "wave", "pigeon" — a wrong choice there does not read as broken, it reads as *confidently wrong*, which is worse in a trust-bearing UI than leaving English.
Every language file therefore starts from a glossary decision for the coined metaphors, applied consistently across all arms, and wants a native reviewer before it ships.
Translate the metaphor where it survives; where it does not, pick one clear concrete word and never drift from it.

### Translating is a proofreader for the English

Adding twelve languages at once found three classes of defect that no amount of re-reading English would have surfaced, because an ambiguity is invisible in the language that has it.

- **`Msg::ClipPill` is ambiguous and should be reworded.** The label is the bare word `clip`, sitting beside `−½ / +½ / 0` in the viewer. **Six of thirteen** independent translators read it as *crop* — Beschnitt, potong, corte, kırpma, kata, ritaglio — when `viewer.rs` defines `clip` as "blown channels black, crushed ones white", a clipping-INDICATOR view. Those six were corrected in place, but the root cause is the English: something like `clipped` or `clip warn` would stop it recurring in every future language. (Māori's `tapahi` has the same crop reading and predates this batch.)
- **`Msg::WaveBarWaving` has a direction only the call site reveals.** "{name} waving" reads equally as us waving them. Three translators resolved it by reading `render.rs`, where it binds to `WavePhase::Ringing` — "their offer reached us", the phase that offers Answer/Decline — and one guessed the opposite and said so. Any variant whose direction is not recoverable from the English wants a doc comment naming its call site.
- **English's unmarked plurals hide required branches.** `MessagesDelivered`, `ContactFleetPinned`, `MessagesSentReceived`, `DiagRecordInspect` and `DiagMeta` do not branch in `en.rs` at all; Russian, Ukrainian and Polish must branch all five on the 1 / 2–4 / 5+ rule with its 11–14 exception. This is the catalog-as-code premise paying off exactly as designed — a data format would have had no place to put that logic.
- **A pre-formatted numeric argument silently forbids agreement.** `Msg::ExposureStops(&str)` receives its value already rendered by `fmt_halves`, so no arm can branch on the number — Russian and Ukrainian independently gave up and wrote `{s} EV` rather than choose wrongly between ступінь/ступені/ступенів. The variant doctrine says parameterized variants carry SEMANTIC values; a `&str` that used to be a number is a formatted value wearing a semantic value's clothes. Any variant whose argument a language might need to agree with should carry the number.

The corollary: a language that needs branching English lacks is a signal the English message is carrying implicit grammar, not a sign the translation is overwrought.
