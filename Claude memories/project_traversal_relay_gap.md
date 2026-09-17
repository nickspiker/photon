---
name: project_traversal_relay_gap
description: 2026-09-17 traversal state — punch pulse now runs for relay-only friends, ring amber = no validated path, Wave pill dims "no direct path"; RELAY MEDIA still unbuilt (Jon Mac↔Nick phone: 3 waves connected, 0 packets); audit of punch tricks vs what's missing
metadata:
  type: project
---

**Field conviction 2026-09-17 (Jon's Mac, home NAT, no own reflexive ↔ Nick's phone, cellular CGNAT):** three waves signalled fine over the relay (offer/answer/hangup) and carried ZERO media both ways — engine tx target 0.0.0.0 / a stranger's 192.168.1.10. Two bugs fixed (photon 6994ef9b→): (1) the seed-registry resolve+punch pulse was gated on a PENDING contact with no address, so a Complete relay-only friend was never resolved or punched from that side (Jon fired 0 punches all day) — now any online contact with no validated path keeps the pulse alive under the same backoff; (2) `contact_conn_tier` painted online-without-validated-path green (the relay flag's "don't override" guard froze it false after a CLUTCH-online edge) — now amber; ShownTier already agreed. Plus (3) the ☎ pill / wave-back read "no direct path" dimmed when only the relay reaches the friend.

**Relay pipe stall observed:** Jon's pipe passed pongs but delivered no pushes for 8 min (18:33→18:41); Nick's answer fell in the hole. Client liveness (120 s) didn't trip — the DO's push queue stalled while TCP stayed up. Robustness item for the pipe rework.

**Punch tricks WE HAVE:** candidate set = global v6 host > v6 reflexive > v4 LAN > v4 reflexive (fgtw traverse crate, sans-io); probes piggyback the presence ping (cadence tapers with idle, held path keeps 20 s keepalive, PATH_TTL 120 s); reflexive from peer-echoed STUN-style ProbeAck `obs` + FGTW announce observation + reseed ladder 0/1/3/9 s on interface change (phones); ping reflection so the candidate-less side gets probed; LAN multicast/broadcast discovery (hairpin workaround); WFD off-grid.

**MISSING (the "have we exhausted our tricks" list, 2026-09-17):** no port prediction for symmetric NAT (no port-delta learning from two observations, no birthday burst); no relay-coordinated SIMULTANEOUS open (both sides punch on the presence cycle independently — a "punch now" nudge over the pipe would align the windows); no UPnP/NAT-PMP/PCP mapping on the home-router side (Jon's Mac could open a static port and become directly reachable to everyone); no IPv6 on Jon's side (v6 needs no punch — worth checking his router); desktop reseed-from-FGTW on interface change appears Android-only (Jon's Mac: "reflexive forgotten … relearning from the next pong" and never re-learned); RELAY MEDIA unbuilt (engine send_media down the pipe when no path validates; worker full-duplex forward exists) — the fallback that would have carried the Jon waves. Nick's stance: avoid relay (Cloudflare cost, identity metadata, latency) — exhaust direct tricks first.

Related: [[project_nat_traversal_relay_gap]], [[project_call_no_ring_incident]], [[relay-asymmetry-ping-reflection]].
