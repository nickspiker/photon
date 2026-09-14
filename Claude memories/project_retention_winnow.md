---
name: project_retention_winnow
description: Retention design SETTLED 2026-09-14 (docs/retention.md): the WINNOW (budgets, edges-not-sweepers) waits for device-sync; custody ladder kept/passing/lofted/discarded — first rungs SHIPPED (star, loft v1, sync law, Vault breakdown)
metadata:
  type: project
---

Retention = the **winnow** (never "GC" — it discards referenced content by policy: size cap AND age window per category, both ceilings, enforced on write/launch edges). Canonical: docs/retention.md.

SHIPPED 2026-09-14 (v0.97.1/.2 dev line): the sync law "authorship propagates, experience doesn't" enforced both ends (delete marker only for own rows and never waves; received markers only tombstone the sender's own rows; edits supersede only their own direction — was spoofable before); action-row pills WRAP (sel_action_extra_h, the sel_meta_h one-frame-settle pattern); `chat.history` linked toggle (default OFF) lists prior edit versions in the selected meta (rows always persisted — braid key material); LOFT v1 (slot 12, incoming pigeons only — local blob_delete, row+refetch stay; en loft / es al palomar / mi whata); Vault page "what holds the space" breakdown (in-memory row-record walk, filter pills everything/waves/pictures/songs/files/★kept, VaultFilter + compute_vault_breakdown).

**Why:** Nick's ask arc — per-chat sizes → allocation budgets ("50 GB to Photon, messages 5 GB or 1 year whichever trims more") → the verbs (keep/loft/discard) → the sync question.

**How to apply:** the winnow's ENFORCEMENT and loft-for-own-uploads wait for the device-sync phase (blob custody must be checkable; row `replicated` flag is NOT blob custody). Decided-not-built: a recipient's keep made BEFORE the author's retraction stands. Graduated eviction (full→preview→thumb→row) is the winnow's mechanism of choice. Related: [[project_vault_roadmap]], [[project_attachments]], [[project_wave_card]].
