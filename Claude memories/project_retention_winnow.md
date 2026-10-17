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
