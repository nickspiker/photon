---
name: project_wave_card
description: "WAVE CARD BUILT 2026-09-09 (PHCALL3 flag day): one row per wave (offer_osc+1, every end edge, typed outcome), recording row REFERENCES it and folds in at render; waveform IS the seek bar (two channels, stops envelope, gross-bucket thumbnail in the row); all/waves/text filter pill"
metadata: 
  node_type: memory
  type: project
  originSessionId: 332790be-f2ba-4740-b64e-33bd0686575a
  modified: 2026-09-09T18:55:52.248Z
---

Wave card BUILT 2026-09-09 (PHCALL3 container flag day, RefKind::Wave=8, ChatMessage.wave/envelope, vault fields wave_out/wave_secs/wave_env, page columns m_wvo/m_wvs/m_wvn/m_wve). Design agreed 2026-09-09 (Nick: "lay it out in your head and do what makes the most sense, not the convention"). Only test waves exist before this; the first real wave is placed once it ships, so no migration/orphan handling.

- ONE row per wave: minted on EVERY end edge at offer_osc+1 with a typed outcome (answered/missed/declined/busy/dropped) + duration. The kept recording (call.audio at offer_osc+2) REFERENCES the wave row (edit-target pattern) and the renderer folds it in; the card exists at hangup, before the transcode finishes ("keeping" state).
- The waveform IS the seek bar: no separate scrubber. ch0 = you above the centreline, ch1 = them below, party colours; tap = play from there / seek; drag scrubs, release = seek edge (no timers); play/stop glyph at the left edge; played bright, unplayed dim, hairline playhead + elapsed/total in dozenal.
- Envelope in STOPS below full scale (u8), computed at transcode (frames are decoded there anyway). 2026-09-09 COLOUR: 4 components per bucket [amp, r, g, b] — amp = height, r/g/b = high-pass energies at 1:8 / 1:4 / 1:1 (running-sum differences, NO FFT; Nick: 'keep it very simple… all high pass'), brightness relative to amp, triple normalised to saturate; container PHCALL4. Thumbnail = one GROSS (144) buckets × 4 per channel in the row; fine envelope 4/s in the container. Drawn OVERSAMPLED like the lumis histogram (coverage-weighted energy mean per column, √fraction tip alpha). 'Self up, they down' is the vertical axis. Appearance tuning still to come.
- Seek primes the Opus decoder from a few slots before the target, never decode-from-zero.
- Filter = three-segment pill in the conversation top bar (all / waves / text), ONE predicate in chat_row_visible (render + jump walk share it), session state only.

**Why:** the two-entries-per-wave symptom (text summary at +1 from a still-ringing sibling on the final hangup + the recording at +2) — a structural fold beats patching mint sites.
**How to apply:** any new call outcome is a wave-row outcome, never a new text row; anything the card needs from the audio is computed at transcode and stored typed (binary at rest), never decoded at draw time. See [[project_voice_calls]], [[feedback_stops_not_db]], [[edges-not-timers]].

**GOTCHA (2026-09-09, shipped broken in v88, fixed 52b823a):** a new history-page column must be declared in `page_schema()` in network/history_pages.rs as well as the builder/parser — the builder VALIDATES, so an undeclared field refuses every seal: sibling row pushes and history pages fail silently (the only symptom was four page tests failing). Run `cargo test --lib network::history_pages` after any page-format change.
