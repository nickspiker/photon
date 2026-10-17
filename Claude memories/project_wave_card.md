---
name: project_wave_card
description: "WAVE CARD shipped 2026-09-12/17: one row per wave with typed outcome, waveform IS the seek bar, wave.env colour envelope, verbatim keep (no transcode), plaid nowhere at rest"
metadata:
  type: project
---

**Row model:** ONE row per wave, minted on every end edge at offer_osc+1 with a typed outcome (answered/missed/declined/busy/dropped) + duration. The kept recording (`wave.audio`, offer_osc+2) references the wave row and the renderer folds it in; the card exists at hangup ("keeping").

**Card:** the waveform IS the seek bar — ch0 = you above the centreline, ch1 = them below, party colours. Unselected = band alone; selected adds header + strip (play/stop · wave back · beam back · export · replicate · discard). A band tap is a row tap; two steps to play. Seek primes the Opus decoder a few slots early. Filter pill (all / waves / text) = one predicate in `chat_row_visible`.

**wave.env:** per-party VSF blob minted at keep on the friend chain: [4, bins] 8-bit tensor = power + RED/GREEN/BLUE voice bands (shift-only RunBox filters, record.rs BandRig); short waves mint none (receiver derives). Render = RMS against a FIXED reference, colour = AGB band ratios to the geometric mean (`wave_fold_coverage` / `wave_fold_colours`).

**Keep (verbatim, 2026-09-17):** repack, never transcode — ch0 = the clean pre-duck archive packets, other channels = the wire packets with fills, holes = zero-length packets. Raw PCM never rests; plaid is re-encoded at receive. Late post-keep fills are not patched into the blob. Container is PHWAVE9 since the rename.

**Why:** two entries per wave (summary + recording) — a structural fold beats patching mint sites.
**How to apply:** a new wave outcome is a wave-row outcome, never a new text row; anything the card needs is computed at keep and stored typed, never decoded at draw time.
**GOTCHA:** a new history-page column must be declared in `page_schema()` (network/history_pages.rs) as well as the builder/parser, or every seal is refused silently; run `cargo test --lib network::history_pages` after page-format changes.
Related: [[project_waves]], [[feedback_stops_not_db]].
