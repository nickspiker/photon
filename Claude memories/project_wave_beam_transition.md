---
name: project-wave-beam-transition
description: "HARD vocabulary rule + the 2026-09-23 flag day: wave = audio (a sound wave), beam = video (a beam of light); \"call\" survives only in the docs/waves.md + docs/lexicon.md passage that retires it, and in platform API names"
metadata:
  type: feedback
---

**Wave = audio, beam = video.** Nick 2026-09-23: *"soundWave Beam of light"*. The rename from "calls" was total that day: module, identifiers, wire bytes, UI strings in every language, docs, Android, installers.

**Why:** a *call* is a circuit a third party holds open, and "nobody kept a copy" was a fact about the price of tape. Every digital call is recorded — by the carrier. Photon has no carrier and keeps its own recording openly, so it does not borrow the word.

**How to apply:**
- Never write "call" for the feature. A wave is voice; a beam is video ([[project_beams]]).
- The word appears only in the "Why not «call»" section of `docs/waves.md` and the retired `## call` entry in `docs/lexicon.md`; `Msg::AboutWaveBeamProse` tells the user the same.
- Platform API names stay as spelled (`CATEGORY_CALL`, `STREAM_VOICE_CALL`, `ConnectionService`, `MODE_IN_CALL`…), and so does code-sense "call" (`call_method`, `callback`, "Caller must zeroize").
- `caller`/`callee` became **`origin`** / **`answer`** (`we_are_origin`, `Direction::OriginToAnswer`, labels `o>a`/`a>o`); prose: "the origin" and "the answering side".
- Dated verbatim quotes ARE rewritten (Nick 2026-09-24: *"I meant waves so do change the quotes too"*); the idiom "someone's call" became "someone's ruling".
- A quote contrasting WIRELINE telephony keeps the word: "these are waves, not wireline calls".

**Flag day (bytes changed, no legacy read):** `WAVE_PREFIX` `photon-wave`; KDF contexts `PHOTON_WAVE_v1`; keep container `PHWAVE9` (every `PHCALL` read arm deleted); spool `PHWAVE1\0`; vault register `wave.spool.<id8>`; attachment `wave.audio` (a beam mints `beam.video`); Android channel `photon.waves`, intents `com.photon.*_WAVE`, JNI `nativeWaveAction`; log tag `WAVE:` (logs before 2026-09-23 say `CALL:`). Paths: `src/wave/`, `wave_ui.rs`, `docs/waves.md`, `tests/wave_media_loop.rs`; `scripts/lib/artifact-gate.sh` allowlists `wave/spool.rs` (a module rename can trip that gate).

Bulk text rewrites with Perl corrupt non-ASCII — see [[feedback_perl_utf8_trap]].
Related: [[project_waves]], [[feedback_spelling]].
