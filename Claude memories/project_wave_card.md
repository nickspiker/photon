---
name: project_wave_card
description: "WAVE CARD: one row per wave; WAVE.ENV EXCHANGE 2026-09-12 (3-channel u8 stochastic tensor VSF blob per party, S0=1 sample, container ships no envelope, cumulative-stack render + tip opacity AA, colouring parked)"
metadata: 
  node_type: memory
  type: project
  originSessionId: 332790be-f2ba-4740-b64e-33bd0686575a
  modified: 2026-09-09T18:55:52.248Z
---

Wave card BUILT 2026-09-09 (PHCALL3 container flag day, RefKind::Wave=8, ChatMessage.wave/envelope, vault fields wave_out/wave_secs/wave_env, page columns m_wvo/m_wvs/m_wvn/m_wve). Design agreed 2026-09-09 (Nick: "lay it out in your head and do what makes the most sense, not the convention"). Only test waves exist before this; the first real wave is placed once it ships, so no migration/orphan handling.

- ONE row per wave: minted on EVERY end edge at offer_osc+1 with a typed outcome (answered/missed/declined/busy/dropped) + duration. The kept recording (call.audio at offer_osc+2) REFERENCES the wave row (edit-target pattern) and the renderer folds it in; the card exists at hangup, before the transcode finishes ("keeping" state).
- The waveform IS the seek bar: no separate scrubber. ch0 = you above the centreline, ch1 = them below, party colours; tap = play from there / seek; drag scrubs, release = seek edge (no timers); play/stop glyph at the left edge; played bright, unplayed dim, hairline playhead + elapsed/total in dozenal.
- WAVE.ENV EXCHANGE (2026-09-12, supersedes the in-container envelope): THREE pyramids per channel — power x², (n0−n1)², ((n0+n1)−(n2+n3))² — S0 = 1 SAMPLE, fold-by-2 at 2^17 cap, so a completed wave ≥2^16 samples lands 2^16..2^17 bins; per-bin MEAN POWER normalized to per-component peak and STOCHASTICALLY quantized to u8 (deterministic splitmix dither — sub-LSB signal survives the render averages). Shipped as a standalone VSF file (section "wave_env": rate/spb/bins u, peak t_f5[3], env p BitPackedTensor 8-bit [3,bins]; read thru read_verified) stored as a blob, minted at keep as a "wave.env" attachment row at offer_osc+3 referencing the wave row, sent on the FRIEND chain + sibling push: each party's half of the card renders from THAT party's own clean-mic tensor. Short wave (<2^16 samples) mints none; the receiver derives all channels from held audio (envelopes_from_blob), same fallback for the far side pre-blob and old recordings. Container writes env_len=0 (V6 layout unchanged); row thumbnails no longer minted.
- RENDER = THE CUMULATIVE STACK (aliasing fix): integer map bin·cols÷bins, every bin adds into exactly one pixel bin, per-pixel count divides — true mean power per column, no interpolation branch, no bin/column beat; height = RMS amp vs −12 dBFS; fractional tip = OPACITY alpha (vertical alias only); bars in party colours (us down, them up); COLOURING FROM THE 3 CHANNELS IS PARKED until Nick approves the aliasing. Cache per (source hash, channel, width).
- COLOUR DOCTRINE ARC (same day): fluor's whole palette routes thru const fn vsf() — authored VSF RGB, identity on macOS (ICC-tagged surface), Q16 matrix + const isqrt bake elsewhere; photon's raw backdrops became VIEWER_BG/CONSENT_BG palette entries.
- Seek primes the Opus decoder from a few slots before the target, never decode-from-zero.
- Filter = three-segment pill in the conversation top bar (all / waves / text), ONE predicate in chat_row_visible (render + jump walk share it), session state only.

**Why:** the two-entries-per-wave symptom (text summary at +1 from a still-ringing sibling on the final hangup + the recording at +2) — a structural fold beats patching mint sites.
**How to apply:** any new call outcome is a wave-row outcome, never a new text row; anything the card needs from the audio is computed at transcode and stored typed (binary at rest), never decoded at draw time. EnvPyramid needs no total up front — a future live in-call band is a refactor, not a redesign. See [[project_voice_calls]], [[feedback_stops_not_db]], [[edges-not-timers]].

**GOTCHA (2026-09-09, shipped broken in v88, fixed 52b823a):** a new history-page column must be declared in `page_schema()` in network/history_pages.rs as well as the builder/parser — the builder VALIDATES, so an undeclared field refuses every seal: sibling row pushes and history pages fail silently (the only symptom was four page tests failing). Run `cargo test --lib network::history_pages` after any page-format change.

2026-09-11 evening: render = RMS amplitude linear against a FIXED reference (−12 dBFS fills the band; never per-channel/per-recording normalised); colour = AGB-style ratios of band power to the geometric mean of the three, brightest channel pinned full (hue = timbre, brightness constant); keep-side Haar tree tracks 7 levels, bands red 188–750 Hz / green 750–3k / blue 3–24k (first cut coloured only 3–24k → dark and red). Nick: "geometric mean, that is how the AGB colour model works".
