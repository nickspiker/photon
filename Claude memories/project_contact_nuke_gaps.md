---
name: project-contact-nuke-gaps
description: "Contact Manage BUILT v106 (2026-09-27): Boot sticks via the booted ledger, clear waves / clear all history; unbuilt: appear offline, mute, unfriend goodbye, date ranges, ask-them-too, discarding row text under braid v2"
metadata:
  type: project
---

Nick 2026-09-27 wants to "nuke a contact so it basically undoes adding them", with or without wiping history fleet-wide (Theresa deleted and it "still persisted").
The resurrection hole (verified): the attest cloud-contacts merge union-added every cloud contact not held locally with roster_updated = now, outranking the Boot tombstone fleet-wide.

**BUILT 2026-09-27 (v106):** `storage::booted` ledger (vault_key "booted", loaded at attest) + manage.rs (`note`/`forget`/`booted_refuses`/`is_booted_party`, `clear_active_history`); `current_roster` re-emits ledger tombstones; `merge_roster_entries` refuses pre-boot live entries (a strictly newer re-add lifts the boot); the cloud merge refuses booted rows and stamps cloud stubs roster_updated = 0; chain-sync refuses booted friendships; Boot rewrites the cloud blob at once; a local add calls `forget_booted`.

**How to apply:** any new path that can re-create a contact (merge, adopt, restore) must consult `booted_refuses`.
**Still unbuilt:** appear offline, mute, unfriend goodbye, date ranges, ask-them-too, and discarding deleted row text under braid v2 ([[project_braid_v2]], [[feedback_discard_not_shred]]). A fleet-wide history wipe needs a conversation-level tombstone (collides with retention.md §5 braid-bound rows). Related: [[project_lifecycle_flows]], [[project_retention_winnow]].
