---
name: project_recording_fills
description: Recording fills SHIPPED 2026-09-10 (5a2e33d): lost windows asked back from the peer live + a post-hangup drain; seq-slotted spool; FILL datagrams on their own chain
metadata:
  type: project
---

Recording fills shipped 2026-09-10 in photon 5a2e33d (docs/calls.md "Recording fills").
What one side lost is what the other side sent and spooled, so the kept wave asks it back: live (eight wanted seqs per FILL datagram every 40 ms, served off the peer's spool by window seq) and in a post-hangup DRAIN (audio off, tail up to the peer's final window count, 2.5 s deadline, both engines exit when both are satisfied).
FILL datagrams = packet.rs FILL_MAGIC 0xC8 sealed under keys::fill_secret's own StepChains; a pre-fill peer never sees them, and a peer that never sent one gets no drain.
Spool records carry [seq][slot] (SEQ_FLAG); served-after frames are FILL_FLAG and the transcode slots them by seq beside arrived windows (record.rs seq lattice keeps a lost window's hole).
The keep transcode joins the engine thread first (the drain writes the spool after stop()); media sinks are generation-counted so a draining engine never clears the next wave's sink.

**Why:** Nick 2026-09-10: "get the missing pieces the other party has… fill in as we go and only lose a second or so of gaps"; the Azie/Nick 15-min call lost 139 windows.
**How to apply:** field-verify with two updated devices (log line `CALL: fills — asked … got … live + … in the drain`); if a recording still shows holes, check the seq lattice re-anchor (rung change across a gap) before the wire. Related: [[project_voice_calls]], [[project_wave_card]].
