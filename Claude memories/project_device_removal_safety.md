---
name: project_device_removal_safety
description: Device-removal safety arc (2026-10-17) — phase 1 wrapper + phase 2 have-set exchange BUILT; phases 3-5 open; design rules for counting holders
metadata:
  type: project
---

Release must never silently take the last copy of an attachment. Canonical write-up: docs/durability.md §7.

- Holders counted per ITEM, never per device (a watch vouches only for what it holds).
- Reply = list of MISSING content hashes, not a bitmap.
- Phase 1 BUILT (8cc9831f): wrapped original keyed by header hb ([[project_chain_text_and_pointers]]).
- Phase 2 BUILT 2026-10-17 (ui/photon_app/holdings.rs): arming Release → worker probes held originals+heads → `holdings_ask` (fleet-key-derived seal) to live siblings → each answers `holdings_missing` off-thread → at-risk lines + non-answering siblings with last-heard age under the Release pill. Second tap still releases. RAW hashes until v134 (presence follows the raw→hb alias).
- OPEN: 3 pull-these-now + persisted remove-when-done; 4 free space / target choice / export; 5 durability cursor for rows. Field test of phase 2 pending (needs a dev build on two devices).

**Why:** Nick 2026-10-17: "let the user know they're removing a device that has the only copy and to wait for sync or offer other options"; "My watch likely doesn't have my movie library".
**How to apply:** new removal work extends holdings.rs; keep presence probes on workers ([[feedback_render_never_touches_vault]]); the fleet key is read from RAM only there.
