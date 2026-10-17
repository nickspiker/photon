---
name: reference-vsf-primary-section
description: "VSF gotcha + its fix: section names live in the header TOC (bare parse gives name \"\") and header-only sections have no body — always read via VsfHeader::primary_section"
metadata:
  type: reference
---

Two VSF wire facts that keep biting hand-rolled readers:
1. Near-form (<1 MB) section bodies are ANONYMOUS — the name lives only in the header TOC, so `VsfSection::parse(...).name` is `""` and any `section.name == "foo"` check silently rejects every real frame (no error, no log).
2. Zero-field sections (pings, acks) encode HEADER-ONLY — no `[` body; bare `VsfSection::parse` errors "Expected '['".

**The fix (2026-07-13):** `VsfHeader::primary_section(data, header_end)` (vsf/src/file_format.rs) resolves the TOC name onto the body, materialises header-only sections, and still fails loudly on truncation. Header-inline values stay on `header.fields`.

**How to apply:** any VSF reader = `VsfHeader::decode` (or `read_verified`) → `header.primary_section(bytes, header_end)`. Never sniff `bytes[header_end] != b'['`, never name-check a bare-parsed section. Writing a NEW reader is exactly when this bites. Pin every wire codec with a build→parse round-trip test.

**Casualties (all fixed):** fgtw `parse_pair_event` (hub push accelerator never fired), the relay pipe's `peel_relay_envelope` (black-holed every forwarded envelope on 0.41.x, 65d7820), LAN discovery beacons (zero parsed fleet-wide for weeks, c57ad78; `lan_discovery_beacon_round_trips` now pins it).
Related: [[project_vsf_canonical_signing]].
