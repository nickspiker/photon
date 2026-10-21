---
name: project_retention_winnow
description: "Retention SETTLED 2026-09-14 (docs/retention.md): the WINNOW waits for device-sync; first rungs shipped (sync law, keep, loft v1, Vault breakdown); eviction keys on RECEIPT (2026-10-07)"
metadata:
  type: project
---

Canonical: docs/retention.md. Retention = the **winnow** (never "GC" — it discards referenced content by policy: a size cap AND an age window per category, enforced on write/launch edges, graduated eviction full → preview → thumb → row).
Shipped 2026-09-14:
- Sync law "authorship propagates, experience doesn't": delete markers only for own rows and never waves; received markers only tombstone the sender's own rows; edits supersede only their own direction.
- The `chat.history` toggle (default off) shows prior edit versions (rows always persist — braid key material).
- LOFT v1 (incoming pigeons only: local blob_delete; the row + refetch stay).
- The Vault page breakdown (`compute_vault_breakdown`, filter pills).
**Eviction keys on RECEIPT, not creation (Nick 2026-10-07):** device-local per-hash `first_here` / `last_wanted` (a re-send refreshes it everywhere); evict bytes only, keep the pointer; never on chain. File pointers hash the RAW bytes (re-sends dedupe).

**Why:** Nick's arc — per-chat sizes → budgets ("50 GB to Photon, messages 5 GB or 1 year whichever trims more") → keep/loft/discard → sync.
**How to apply:** winnow enforcement and loft-for-own-uploads wait for device-sync (blob custody must be checkable; a row's `replicated` flag is not blob custody). A recipient's keep made before the author's retraction stands (decided, not built). Related: [[project_vault_roadmap]], [[feedback_discard_not_shred]].

**2026-10-21 DECIDED (Nick): retention is PER DEVICE, budget = "a reasonable amount of current free space"** — no per-category size numbers; a free-space floor per device, eviction at the write edge (oldest by arrival, unstarred; beams, then waves). Recorded in TICKETS.md retention entry. Beam video recording waits for this (see [[project_beams]]).
**2026-10-21 BUILT v1 (space.rs):** floor = disk_free + 2×vault dead space ≥ volume/8 (vault grows by doubling, never shrinks, mirrored ×2). Edges: session up, recording kept, recording landed. Below floor: recordings stop replicating in (`recording_wanted`), and oldest unstarred recordings are evicted ONLY after a sibling answers a holdings Ask saying it holds them (reuses holdings.rs frames; `space_answer` routed before `holdings_answer`). Evicted hashes → device-local `space.evicted` (hR); any `attach_fetch` (deliberate) un-evicts. Single device / no sibling online → keeps everything. Beams join when beam recording exists.
