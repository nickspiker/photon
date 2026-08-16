---
name: relay-asymmetry-ping-reflection
description: "2026-08-13 Mary-relay/Nick-direct asymmetry: self-claimed :4383 public records + reflection only firing on probes; FIX 271c76c = ping reflection; deeper issue OPEN: announce publishes bind port, not NAT-observed port"
metadata: 
  node_type: memory
  type: project
  originSessionId: 81588914-5914-4600-bb98-72cc4fae2260
  modified: 2026-08-13T06:19:41.207Z
---

Field 2026-08-13: Mary showed RELAY from both Nick devices while Nick showed DIRECT from Mary's device. Diagnosis from logs (fe46a74b=MacBook, 1be949c1=Android):

- BOTH Nick devices announce "we look like 5.148.42.186:4383" — identical public record. The announce publishes the SERVER-OBSERVED IP + the SELF-REPORTED BIND PORT (4383), not a NAT-observed mapping. Two hosts can't share one mapping: Mary's punch lands on whichever device the router forwards 4383 to → she validates "direct to Nick" against ONE device.
- Nick's devices held only Mary's foreign-LAN row (10.21.67.185) — no public — so their punches probed an unreachable 10.x forever ("pending relay (M2)"). Mary's record has the same self-claimed-port disease.
- The probe-reflection (built 2026-08-11 for one-directional validation) never fired: it triggers on inbound PROBES, but a peer with a validated path keeps it warm with PINGS — the steady-state signal carries no trigger.

FIX photon 271c76c (awaiting field verify): ping reflection — a signature-verified DIRECT ping from a known device proves its src_addr is a working return path; reflect a probe at it (same reverse_probed 60s dedup as the probe arm; relay-injected pings carry the unspecified sentinel and are excluded by is_bogus_addr). The reflected probe's ack validates our direction AND its observed-addr echo feeds ReflexiveLearned → the next announce publishes the TRUE UDP mapping → records heal fleet-wide from one direct ping. Also: punch candidate lists bogus-filtered (a poisoned-era contact.ip put 0.0.0.0:4383 in a live probe set).

SECOND FIX photon 30e81b6 (Nick's call: "try the UDP observed and if it works, use that"): the reflect-beside-pings bootstrap. The receive half all existed (open-tier Reflect serve, ReflexiveLearned, anti-poison quorum needing 2 distinct echoes, the tick re-publish edge) — but NOTHING ever sent a ReflectRequest. Now: StatusChecker.needs_reflect (default true) → every direct ping carries a Reflect; first quorum-adopted ReflexiveLearned clears the flag; OurLanAddrObserved interface-change re-arms it. The record then re-publishes with the TRUE UDP mapping.

STILL OPEN (smaller now): the fgtw.org server-side announce row ("we look like", TLS-observed) still carries the bind port — the SIGNED peer-record path is healed, but refresh_contact_addrs_from_peers-style consumers of the server row keep the fiction until a similar cutover. Watch next round: "TRAVERSE: reflecting probe at pinger", "our reflexive address =" with a NON-4383 port, records re-published per device with diverging mappings, Mary's row going direct. Related: [[self-pair-sibling-row]], [[lane-rotation-wedge-heal]].
