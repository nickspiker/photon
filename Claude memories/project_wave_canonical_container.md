---
name: project_wave_canonical_container
description: "LOCK container decided 2026-09-24: the wave IS the frame set named by grid index, rows are SPANS in a conversation table, canonical order = grid index then handle proof, ONE signature per party at the truing-up, no mix at rest"
metadata:
  node_type: project
  type: project
---

Nick's LOCK spec (universal-time alignment for wave/beam) landed 2026-09-24 and the container question was settled in the same conversation. Doctrine now written into docs/waves.md, "The canonical wave" section.

**Nick's framing, verbatim:** *"treat it like a regular conversation and order it by eagle time proper, each frame of audio is aligned to the second boundary and are sorted on each end by handle proof… we have record of every packet we attempted to send at said bitrate we were spooling at so the code rate tracks and we only have to fill in minor holes without weird jumps in quality… No mix stored, no reason, all sides need to hold the same copy when we're done. playback is when mix occurs."*

DECIDED:
- The artifact is the FRAME SET keyed by grid index (`k0`, `k0 % 960 == 0`, 50 slots/sec/channel from the top of each Eagle second). PCM is a rendering, never the artifact — PLC, DAC drift and AEC differ per device by construction.
- Canonical order = grid index, then capturing party's handle proof. Both sides derive it with no exchange, which is what makes a content hash NAME the recording.
- Identity is CONVERGENT: fills move both sides toward the union; equal coverage ⇒ equal hash ⇒ bit-identical forever, because no device may invent a frame. Holes are typed absences at a known rung.
- The sender's spool manifest (every attempted packet + its rung) defines "complete" and keeps fills at the neighbours' rung — no quality steps.
- Reconstruction is a PURE function of the frame set: decode in contiguous spans, a hole TERMINATES the run and the next starts on a reset decoder. Otherwise one lost packet poisons decoder state forever and two devices never re-converge. Runs off-thread (no vault, no ambient state).
- No mix at rest, one channel per party — this is what serves transcription and per-party levels.
- SIGNATURE SCOPE (my call, Nick delegated): ONE signature per party per wave at the truing-up over its channel's root + manifest. NOT per packet — 64 bytes × 50/s = 25.6 kbps, which exceeds the whole sublight rung (16 kbps) on exactly the bad links that force that rung. Live fills are already authenticated by the media plane (per-direction subkeys, sequence number as nonce); sibling fills by the braid. Pre-signature a wave is a spool, not a recording — matches the Keep/Delete ticket.
- ROWS ARE SPANS, NOT FRAMES: 10 min × 50/s × 2 ch = 60k frames; one rārangi row each would build a 60k-pk catalog that re-encodes as it grows — the exact cost convicted in [[project_history_early_stop_blindness]]. One row per contiguous run, keyed by its first grid index. Then the conversation machinery applies unchanged: fills = backfill, held row keys = the coverage map, fleet replication already carries it.

DECIDED 2026-09-25 (Nick): ONE SPOOL PER IDENTITY — each party's channel its own vault object, frames by absolute 5 ms grid name k0 (k0 % 240 == 0), sender slips/stretches to true time at the source, first frame zero-padded (never wait for a boundary), far channel stored under the SENDER's wire names so all channels align at mic time → export/replay = channels side by side. Written in docs/waves.md "One spool per identity". This supersedes the interleaved single spool (spool.rs chan tag).
PROGRESS (2026-09-25): spec VERBATIM at docs/lock.md, implementer status at its end — read that first. BUILT: vsf Eagle+grid; TrueClock (network/true_clock.rs + time_base, lock-free snapshot for audio callbacks); capture stamps on the boot clock (AAudio Boottime, desktop callback − cpal capture delay); wave/align.rs aligner (rate fit + slip PI loop, 5 ms grid frames, first padded); W1 engine = aligner in TX, u32 frame numbers on every datagram + fills, both channels spooled at sample_to_eagle(k0) (WIRE FLAG DAY). NEXT: one vault object per identity at keep (PHWAVE9 still interleaves), receiver playout vs true time (§7), DAC loop, clap test. Nick's Tukutahi draft (video+audio timing/rate declaration) saved verbatim at docs/tukutahi.md — "food for thought", not yet discussed. nunc fixed that day (NTP rtt/2 bias, NTS whole seconds).
EARLIER NOTE: none of the LOCK spec is implemented — implementation order is in the spec (vsf Eagle fixes + `grid` module first). Note the log pulled that day held NO wave session, so the sig-scope call rests on the loss-rate loop's own target (PID to under 1/256, docs/waves.md) rather than measured wave loss. See [[project_wave_card]], [[project_recording_fills]], [[project_waves]].
