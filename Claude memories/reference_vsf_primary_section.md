---
name: reference-vsf-primary-section
description: "VSF gotcha + its fix: near-form sections parse ANONYMOUS and header-only sections have no body — always read via VsfHeader::primary_section, never bare VsfSection::parse + name check"
metadata: 
  node_type: memory
  type: reference
  originSessionId: d2067fd8-576f-40e0-987a-80c59aedb715
---

Two VSF wire facts that repeatedly bit hand-rolled readers (root-caused 2026-07-13):
1. Near-form (<1MB) section bodies are ANONYMOUS — the name lives only in the header TOC, so `VsfSection::parse(...).name` is `""` and any `section.name == "foo"` check silently rejects every real frame.
2. Zero-field sections (pings, acks, empty registries) encode HEADER-ONLY: a name-only TOC entry and NO `[` body at all — bare `VsfSection::parse` errors "Expected '['".

**The fix (built 2026-07-13): `VsfHeader::primary_section(data, header_end)`** in vsf/src/file_format.rs — resolves the TOC name onto the parsed body, materialises header-only sections as zero-field sections of their TOC name, and still fails loudly on truncation (TOC claims bytes/children but no body present). Header-INLINE values (`(name:v1,v2)` TOC entries) stay on `header.fields`, not section fields.

**How to apply:** any new VSF reader = `VsfHeader::decode` (or `read_verified`) → `header.primary_section(bytes, header_end)`. Never sniff `bytes[header_end] != b'['` and never name-check a bare-parsed section.

Casualties found in the sweep (all migrated): photon protocol.rs hand-rolled header-only fast path + allowlist (ping/pong/pb_req/av_req/reflect/punch); the copy-pasted name-fallback in fgtw client parse_section, photon peer_updates.rs, status.rs, protocol.rs parse_section_after_header (now a shim); and the big one — **fgtw pair.rs parse_pair_event name-checked a bare-parsed section, so the hub push accelerator NEVER fired** and the poll cadence silently carried every pairing ceremony (the observed "bind landed but the device sat minutes on the old timeout" was this, worked around at the time by faster polling). Worker acks (fleet_ack etc.) were unparseable-but-unread the whole time; now genuinely parseable.

RECURRENCE (2026-07-23): the trap bit NEW code too — the relay-pipe `peel_relay_envelope` (network/fgtw/relay.rs) did a bare `VsfSection::parse` + `section.name == "relay"` check, so `name == ""` dropped EVERY forwarded envelope: "PIPE: ← 443B dropped — not a valid signed relay envelope", black-holing the entire pipe data plane on 0.41.x even with both peers updated. Fixed via `header.primary_section` @ 65d7820, with a `peel_roundtrip` regression test. Lesson: writing a NEW VSF reader is exactly when this bites — reach for primary_section reflexively, never `VsfSection::parse` + a name check.
Related: [[project-pairing-v2]], [[project-vsf-canonical-signing]].
