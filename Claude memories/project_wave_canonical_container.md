---
name: project_wave_canonical_container
description: "LOCK 2026-09-24/25: a wave IS its frame set named by grid index, one spool per identity, rows are SPANS, ONE signature per party at truing-up, no mix at rest; spec docs/lock.md"
metadata:
  type: project
---

Doctrine lives in docs/waves.md ("The canonical wave", "One spool per identity"); the LOCK spec is verbatim at docs/lock.md with implementer status at its end — read that first.

Nick: *"treat it like a regular conversation and order it by eagle time proper… No mix stored, no reason, all sides need to hold the same copy when we're done. playback is when mix occurs."*

**Decided:**
- The artifact is the FRAME SET keyed by grid index (5 ms grid, k0 % 240 == 0 from the top of each Eagle second). PCM is a rendering, never the artifact.
- Canonical order = grid index, then the capturing party's handle proof — both sides derive it, so a content hash NAMES the recording. Equal coverage ⇒ bit-identical; no device may invent a frame; holes are typed absences.
- Reconstruction is a pure function of the frame set: a hole terminates a decode run; the next run starts on a reset decoder.
- One spool per identity (2026-09-25): each party's channel its own vault object under the sender's wire names, aligned at mic time; first frame zero-padded.
- SIGNATURE: one per party per wave at truing-up over channel root + manifest, never per packet (64 B × 50/s exceeds the 16 kbps rung).
- Rows are SPANS (one row per contiguous run, keyed by first grid index), never one row per frame (the catalog cost of [[project_history_early_stop_blindness]]).

Built 2026-09-25: vsf Eagle + grid, TrueClock, boot-clock capture stamps, `wave/align.rs` aligner, frame numbers on every datagram (wire flag day).
Next (per docs/lock.md): one vault object per identity at keep (PHWAVE9 still interleaves), receiver playout vs true time, DAC loop.
Related: [[project_wave_card]], [[project_playout_l_slips]].
