---
name: project-wave-beam-transition
description: "HARD vocabulary rule + the 2026-09-23 flag day: wave = audio (a sound wave), beam = video (a beam of light); \"call\" survives only in the docs/waves.md + docs/lexicon.md passage that retires it, and in platform API names"
metadata:
  type: feedback
---

**Wave = audio, beam = video.** Nick 2026-09-23, on being asked which metaphor anchors which: *"soundWave Beam of light"*. The transition from "calls" to "waves" was done in its entirety that day — module, identifiers, wire bytes, UI strings in all 16 languages, docs, Android, installers.

**Why:** a *call* is a circuit a third party holds open, and the social contract around it — that nobody kept a copy — was a fact about the price of magnetic tape, not about telephony. Data IS the storage medium now, so carrying a digital conversation and writing it down are the same operation: every digital call is recorded, by the carrier rather than by either speaker. Photon has no carrier and keeps its own recording openly, so it does not get to borrow the word.

**How to apply:**
- Never write "call" for the feature. A wave is voice; a beam is video (designed, not built — the buttons are stubs named `beam_btn` / `beam_back_btn`).
- The word appears in exactly two sanctioned places: the "Why not «call»" section of `docs/waves.md`, and the retired `## call` entry in `docs/lexicon.md`. `Msg::AboutWaveBeamProse` carries the same point to the user, in every language.
- Platform API names stay as the platform spells them: `CATEGORY_CALL`, `STREAM_VOICE_CALL`, `ConnectionService`, `sym_action_call`, `MODE_IN_CALL`. So does code-sense "call" (`Called from the recv worker`, `Caller must zeroize`, `call_method`, `callback`). Both are fine; neither is the feature.
- The `caller`/`callee` pair became **`origin`** / **`answer`**: `we_are_origin`, `origin_nonce` / `answer_nonce`, `Direction::OriginToAnswer` / `AnswerToOrigin` (labels `o>a` / `a>o`). In prose: "the origin" and "the answering side".
- **Dated verbatim quotes ARE rewritten** (Nick 2026-09-24, overruling the first pass's instinct to preserve them: *"Yeah, I meant waves so do change the quotes too"*) — he said "calls" then because that was the word then, and he meant the thing now called a wave, so the quote records the meaning, not the spelling. `"my calls from here to Pennsylvania"` → `"my waves …"`. The idiom "someone's call" = someone's decision became "someone's ruling".
- A quote that names WIRELINE telephony as the contrast keeps the word, because that is the retiring use, not the feature: `"these are waves, not wireline calls"` stands as Nick said it.

**The flag day (Nick's choice: bytes change, no legacy read).** Every at-rest and on-wire name moved together, so a build on one side cannot signal or key with a build on the other — the whole fleet crosses at once:
- lane content marker `WAVE_PREFIX`: `photon-call` → `photon-wave` (a straddling build reads the other's signal rows as ordinary text and shows nothing)
- KDF contexts: `PHOTON_CALL_v1`/`v2` → `PHOTON_WAVE_v1` (secret, recording fill, direction step0, step, express signal)
- keep container: `PHCALL7`/`PHCALL8` → **`PHWAVE9`**, one magic, body byte-identical; every `PHCALL` read arm deleted, so recordings kept before today do not open
- spool container `PHWAVE1\0`, spool temp files `wavespool-*.tmp`, vault register `wave.spool.<id8>`
- attachment row filename `call.audio` → `wave.audio` (beside the existing `wave.env`); a beam will mint `beam.video`
- Android: notification channel `photon.waves`, intents `com.photon.{ANSWER,DECLINE,INCOMING}_WAVE` + `WAVE_ACTION`, JNI `nativeWaveAction`
- log tag `CALL:` → `WAVE:` (matters when reading logs from before the flag day — see [[reference_log_pull]])
- the `wave_secret` KAT in `wave/keys.rs` was regenerated independently with `b3sum --derive-key` (verified against the old literal first, so the method is proven): `48be713c8a80427eaf77732d8940b937229818d8e7370236625371f3cd04132b`

Paths: `src/call/` → `src/wave/`, `call_ui.rs` → `wave_ui.rs`, `docs/calls.md` → `docs/waves.md`, `tests/call_media_loop.rs` → `tests/wave_media_loop.rs`. The artifact gate's loose-file allowlist names `wave/spool.rs` now (`scripts/lib/artifact-gate.sh`) — a module rename can trip that gate before rustc ever runs.

Related: [[project_waves]], [[project_wave_card]], [[project_voice_calls]] is gone (renamed), [[feedback_spelling]].

**Trap worth remembering:** a Perl rewriter with `:encoding(UTF-8)` I/O but no `use utf8;` double-encodes any literal non-ASCII in its replacements, and silently never matches patterns containing one. It cost four mojibake comment lines here. Do the bulk text passes in Python, or scan afterwards for a UTF-8 lead byte followed by a continuation byte read as two characters.
