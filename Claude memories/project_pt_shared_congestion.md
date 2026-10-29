---
name: project_pt_shared_congestion
description: 2026-10-29 PT path pace BUILT — one delay-based send window per path shared by its transfers (replaced blast-256); field test pending
metadata:
  type: project
---
2026-10-29 23:01, Nick's MacBook: `DIAG: log submit failed … error sending request for url (https://fgtw.org/)` and `SETTINGS: push failed: FGTW didn't answer in time`, while it served Josh a 149-chunk (~39 MB) attachment direct.
Cause: each large PT transfer opened with a 256-packet blast (a 256 KB chunk is 258 packets, so every chunk was one blast) and grew a send ratio on loss alone; thirteen at once (`STREAMS_IN_FLIGHT`, a stream-id safety window, not congestion control) held a standing queue on the uplink (RTT 40 ms floor → 70-115 ms, no loss) and the one HTTPS flow starved.
The old completion log's "100% util" meant no retransmits and "win 186" a send ratio of 1.86 — neither measured the link; timeout retransmits were not counted at all.

BUILT 2026-10-29 (Nick: "Chunk pacing -> delay based please"): `PathPace` in src/network/pt/window.rs, one per path, shared by every transfer on it; window moves by (base + PACE_TARGET 32 ms) / rtt per round trip (each ACK takes 1/in_flight of the step in log2), grows only while it binds, halves on loss once per ACK-less stretch, empty path admits one packet (the ACK clock's seed). Pace closes when its path idles, so base RTT is re-learned each busy spell. Karn's rule in FlightTracker (`resent` never timed). Timeout retransmits now counted. The "PT: path … idle — its pace closes at a window of N packet(s), base RTT M ms" line is the field readout.

**Why:** bulk transfers must yield to other traffic on the uplink (FGTW calls, waves, other apps).
**How to apply:** field test pending — check the idle line's window/base on a big serve, and that FGTW calls succeed during it; delay-based yields to loss-based TCP elephants, so a long upload by another app slows attachments by design. Related: [[project_beam_standing_queue]].
