---
name: relay-asymmetry-ping-reflection
description: "CLOSED 2026-08-13: one side relay, the other direct, because announces published the bind port and reflection never fired on pings; ping reflection + reflect-beside-pings fixed the signed records"
metadata:
  type: project
---

Field 2026-08-13: Emma saw RELAY to both of Nick's devices while Nick saw DIRECT to her.
- Both Nick devices announced the same `public-ip:4383` — server-observed IP + self-reported BIND port, not the NAT mapping — so a punch reached only whichever device the router forwarded to.
- Probe reflection triggered only on inbound probes, but a validated path stays warm with PINGS.

**Fixed:**
- photon 271c76c: ping reflection — a signature-verified direct ping proves a return path; reflect a probe at it, whose observed-address echo feeds `ReflexiveLearned` → the next announce publishes the TRUE mapping.
- photon 30e81b6: `needs_reflect` — every direct ping carries a Reflect until a quorum-adopted reflexive address lands; an interface change re-arms it.

The fgtw.org server-side announce row still carried the bind port at the time; see [[project_address_ports]] (2026-10-15) for the later port work.
Related: [[project_traversal_relay_gap]].
