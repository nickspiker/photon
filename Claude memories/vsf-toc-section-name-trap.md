---
name: vsf-toc-section-name-trap
description: "VSF section NAMES live in the header TOC — a bare VsfSection::parse returns name \"\" and any == \"name\" check silently fails; use header.primary_section"
metadata: 
  node_type: memory
  type: project
  originSessionId: 296bef77-7c97-45b8-8d90-dc492f93e557
  modified: 2026-08-11T14:41:04.116Z
---

A VSF document's section NAME rides the header TOC (near-form), not the section body. `VsfSection::parse(bytes, &mut ptr)` on the body returns `section.name == ""`, so any hand-rolled `section.name == "foo"` check silently rejects EVERYTHING — no error, no log.

**Why:** three separate subsystems were black-holed by this exact shape: the relay pipe, the hub push accelerator, and (2026-08-11) LAN discovery — beacons received on every device, zero parsed, fleet-wide, for an unknown number of weeks; same-LAN peers could only learn each other from registry records (WAN-only for a multi-homed phone) and parked on relay.

**How to apply:** resolve sections via `header.primary_section(bytes, header_end)` (relay.rs carries the canonical comment). When touching any VSF parse path, check for bare `VsfSection::parse` + name comparisons — and pin every wire codec with a build→parse round-trip unit test (the beacon had none; `lan_discovery_beacon_round_trips` in udp.rs would have caught the drift the day it happened). Fixed in photon c57ad78. Related: [[persist-findings-early]].
