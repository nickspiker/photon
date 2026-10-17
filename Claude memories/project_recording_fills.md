---
name: project_recording_fills
description: "Recording fills SHIPPED 2026-09-10 (live + post-hangup drain; live field-verified both ways); spool keeps RAW mic + verdict; per-channel Opus 128k archive; next = offline canceller at keep"
metadata:
  type: project
---

Canonical: docs/waves.md "Recording fills". What one side lost, the other side sent and spooled, so the kept wave asks for it back:
- Live: FILL datagrams (packet.rs `FILL_MAGIC` 0xC8, sealed under `keys::fill_secret` StepChains) ask for eight wanted seqs every 40 ms, served off the peer's spool by window seq.
- Drain after hangup: audio off, tail up to the peer's final window count, 2.5 s deadline; tail wants come from the peer's draining count, two goodbye heartbeats before exit (8947821).
- Spool records carry [seq][slot]; served frames are FILL_FLAG; record.rs's seq lattice keeps a lost window's hole. The spool keeps the RAW mic (PROC_FLAG [gain_q8][verdict] + PCM) beside the wire copy; keep builds the local channel from raw. Archive: one mono Opus stream per channel at 128 kbps.

**Why:** Nick 2026-09-10: "get the missing pieces the other party has… fill in as we go and only lose a second or so of gaps".
**How to apply:** if a recording shows holes, check the seq lattice re-anchor (a rung change across a gap) before blaming the wire; next = offline canceller/un-duck at keep from the raw + verdict records ([[project_incall_learner]]).
