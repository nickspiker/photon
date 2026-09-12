---
name: project_wave_card
description: "WAVE CARD: one row per wave, waveform IS the seek bar; ENVELOPE PYRAMID PHCALL6 flag day 2026-09-11 (exact streaming u64 bins, Haar colour tree, power-fold render, variable env_len)"
metadata: 
  node_type: memory
  type: project
  originSessionId: 332790be-f2ba-4740-b64e-33bd0686575a
  modified: 2026-09-09T18:55:52.248Z
---

Wave card BUILT 2026-09-09 (PHCALL3 container flag day, RefKind::Wave=8, ChatMessage.wave/envelope, vault fields wave_out/wave_secs/wave_env, page columns m_wvo/m_wvs/m_wvn/m_wve). Design agreed 2026-09-09 (Nick: "lay it out in your head and do what makes the most sense, not the convention"). Only test waves exist before this; the first real wave is placed once it ships, so no migration/orphan handling.

- ONE row per wave: minted on EVERY end edge at offer_osc+1 with a typed outcome (answered/missed/declined/busy/dropped) + duration. The kept recording (call.audio at offer_osc+2) REFERENCES the wave row (edit-target pattern) and the renderer folds it in; the card exists at hangup, before the transcode finishes ("keeping" state).
- The waveform IS the seek bar: no separate scrubber. ch0 = you above the centreline, ch1 = them below, party colours; tap = play from there / seek; drag scrubs, release = seek edge (no timers); play/stop glyph at the left edge; played bright, unplayed dim, hairline playhead + elapsed/total in dozenal.
- ENVELOPE PYRAMID (PHCALL6 flag day 2026-09-11, replaced the fixed 65536-grid whose sub-pitch bins WERE the chunking): EnvPyramid streams every sample into fixed-integer bins (S0 = 1024 samples = 21.3ms, one pitch period), fold-by-2 on hitting ENV_CAP = 2^17 (u64 sums+counts ADD, bit-exact), stores whatever env_len it ends on (variable u32, readers honour it; nchan rides the wave_env cache, never inferred from length). COLOUR = top three detail levels of the decimated binary sum tree (Haar): blue = pair diff (per 2 samples), green = pair-sum diff /2 (per 4), red = 4-sum diff /4 (per 8) — same ≤2^16 scale, squared into the same bins. RENDER: store stops, fold in POWER (decode LUT → square → resample → root), display in stops; colour JOINT-scale normalised in linear per party (one min..max across all three bands); two independent party passes (them ch1 up, us ch0 down, nothing shared); wave_fold_cache holds FINISHED per-column (stops, colour u32) — rgb_colour's Rec.2020 matrix paid once, capped 128 clear-all. PHCALL4/5 read support DELETED (house flag-day rule; Nick: old previews looked bad). Tests pin fold exactness + octave exclusivity.
- Seek primes the Opus decoder from a few slots before the target, never decode-from-zero.
- Filter = three-segment pill in the conversation top bar (all / waves / text), ONE predicate in chat_row_visible (render + jump walk share it), session state only.

**Why:** the two-entries-per-wave symptom (text summary at +1 from a still-ringing sibling on the final hangup + the recording at +2) — a structural fold beats patching mint sites.
**How to apply:** any new call outcome is a wave-row outcome, never a new text row; anything the card needs from the audio is computed at transcode and stored typed (binary at rest), never decoded at draw time. EnvPyramid needs no total up front — a future live in-call band is a refactor, not a redesign. See [[project_voice_calls]], [[feedback_stops_not_db]], [[edges-not-timers]].

**GOTCHA (2026-09-09, shipped broken in v88, fixed 52b823a):** a new history-page column must be declared in `page_schema()` in network/history_pages.rs as well as the builder/parser — the builder VALIDATES, so an undeclared field refuses every seal: sibling row pushes and history pages fail silently (the only symptom was four page tests failing). Run `cargo test --lib network::history_pages` after any page-format change.

2026-09-11 evening: render = RMS amplitude linear against a FIXED reference (−12 dBFS fills the band; never per-channel/per-recording normalised); colour = AGB-style ratios of band power to the geometric mean of the three, brightest channel pinned full (hue = timbre, brightness constant); keep-side Haar tree tracks 7 levels, bands red 188–750 Hz / green 750–3k / blue 3–24k (first cut coloured only 3–24k → dark and red). Nick: "geometric mean, that is how the AGB colour model works".
