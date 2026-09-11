---
name: project_recording_fills
description: Recording fills SHIPPED 2026-09-10 (5a2e33d, drain fixed 8947821): live fills field-verified both ways (Nick/Emma 5/5 and 89/89); spool keeps RAW mic + verdict (PROC_FLAG); archive PHCALL5 per-channel Opus 128k
metadata:
  type: project
---

Recording fills shipped 2026-09-10 in photon 5a2e33d (docs/calls.md "Recording fills").
What one side lost is what the other side sent and spooled, so the kept wave asks it back: live (eight wanted seqs per FILL datagram every 40 ms, served off the peer's spool by window seq) and in a post-hangup DRAIN (audio off, tail up to the peer's final window count, 2.5 s deadline, both engines exit when both are satisfied).
FILL datagrams = packet.rs FILL_MAGIC 0xC8 sealed under keys::fill_secret's own StepChains; a pre-fill peer never sees them, and a peer that never sent one gets no drain.
Spool records carry [seq][slot] (SEQ_FLAG); served-after frames are FILL_FLAG and the transcode slots them by seq beside arrived windows (record.rs seq lattice keeps a lost window's hole).
The keep transcode joins the engine thread first (the drain writes the spool after stop()); media sinks are generation-counted so a draining engine never clears the next wave's sink.

Field 2026-09-10 (Nick/Emma second wave): live fills worked both ways (Nick asked 5 got 5; served Esme 89, 0 nacked); the DRAIN failed both ways — the hung-up-on side exited at 0 ms before its tail wants (stale live count, exit check before the tail), the other waited the 2.5 s deadline "peer silent". Fixed 8947821: tail wants from the peer's draining count only, satisfied needs peer_draining, two goodbye heartbeats before exit. Same commit: spool keeps the RAW mic (PROC_FLAG [gain_q8][verdict] + PCM) beside the wire copy; keep builds the local channel from raw; PHCALL5 = nchan mono Opus packets per slot at 128 kbps (V4 still opens); wave card folds stops once per width (wave_fold_cache). Next: offline canceller/un-duck at keep from the raw + verdict records.

**Why:** Nick 2026-09-10: "get the missing pieces the other party has… fill in as we go and only lose a second or so of gaps"; the Azie/Nick 15-min call lost 139 windows.
**How to apply:** field-verify with two updated devices (log line `CALL: fills — asked … got … live + … in the drain`); if a recording still shows holes, check the seq lattice re-anchor (rung change across a gap) before the wire. Related: [[project_voice_calls]], [[project_wave_card]].
