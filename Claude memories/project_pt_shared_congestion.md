---
name: project_pt_shared_congestion
description: 2026-10-29 Mac log submit failed — PT's 13 per-peer streams each run their own window, flooding the uplink and starving FGTW HTTPS; shared window PROPOSED, not built
metadata:
  type: project
---
2026-10-29 23:01, Nick's MacBook: `DIAG: log submit failed … error sending request for url (https://fgtw.org/)` and `SETTINGS: push failed: FGTW didn't answer in time`, while it served Josh a 149-chunk (~39 MB) attachment direct.
PT already paces large transfers by completion edge (`STREAMS_IN_FLIGHT = 13` per peer, src/network/pt/mod.rs), but that window exists for stream-id safety, not congestion: each of the 13 transfers runs its own window ("100% util, 0 retx"), and RTT rose from a 40 ms floor to 70-115 ms — a standing queue on the uplink that crowded out the one HTTPS flow.
The 22:32 relay failure the same night was a separate network reset ("Connection reset by peer"), not this.

**Why:** a big attachment serve makes every FGTW call (log submit, settings push, relay) fail for its duration.
**How to apply:** the fix Nick has to approve is ONE congestion window per peer (ideally per uplink) shared across its streams, delay-based like the beam standing-queue verdict ([[project_beam_standing_queue]]); a fixed chunk cap is a magic number (AGENT.md), not the answer.
