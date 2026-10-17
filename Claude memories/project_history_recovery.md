---
name: project_history_recovery
description: "History sync: friend backfill (2026-07-09, E2E verified) + fleet sync between own devices (2026-07-16) — paged hist_req/hist_page, history key, live push, roster CRDT"
metadata:
  type: project
---

Storage-layer sync, NOT a messaging replay — the live braid never sees it.
- **Friend backfill (4ca82ed, E2E verified 2026-07-09):** after a fresh CLUTCH seals, the reset device pulls history FROM THE FRIEND newest-first, paged (head page now, trickle to completion, scrollback jumps the queue). Pages are sealed outside the ratchet under `crypto::clutch::derive_history_key` (derived once at ceremony birth, zeroized on re-key/delete). Recovered outgoing rows flip to delivered; rows are marked `recovered`.
- **Fleet sync (c6d6285, 2026-07-16):** the same codec under the FLEET key when the sender is a fold-trusted sibling; sibling pages merge verbatim (no direction flip). Tokens resolve by derivation from party ids, so roster-merged contacts backfill before any CLUTCH. ONE conversation cursor (hist_oldest/hist_complete in contact state) spans both sources.
- **Live push:** send/receive/self-note/ACK push rows to online siblings as unsolicited fleet-key pages; gossip one hop, never back to the sender; `delivered` is monotonic.
- The roster CRDT rode the same commit (pull-merge-push, roster_updated LWW, tombstones honoured).
- Code: src/network/history_pages.rs (key-agnostic codec), hist_req/hist_page in fgtw/protocol.rs (packet-acked in both RX branches — an un-acked reliable type HOL-blocks chat), `drive_history_recovery`, `kick_fleet_history_sweep`.

**Why:** a reset or new device must regain its history from peers — the fleet (and friends) is the backup.
**How to apply:** early-stop needs divergence evidence ([[project_history_early_stop_blindness]]); durability doctrine is docs/durability.md.
