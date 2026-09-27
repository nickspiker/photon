---
name: project-contact-nuke-gaps
description: "2026-09-27 survey — Boot is the only contact removal (roster tombstone, ostracism not erasure); the cloud contacts blob resurrects booted contacts fleet-wide on the next attest (verified); no delete-conversation feature exists; \"nuke contact\" unbuilt"
metadata:
  node_type: memory
  type: project
  originSessionId: 87fd33b1-f610-46f9-b464-4fe0dd1e3280
  modified: 2026-09-27T23:09:37.366Z
---

Nick 2026-09-27 wants a way to "nuke a contact so it basically undoes adding them", with or without wiping history fleet-wide (Theresa deleted and it "still persisted").
Today: Boot (contact panel Manage → two taps, sync.rs boot_active_contact) mints ONE roster tombstone, deletes contact state/keypairs/slots + chains on this device, rewrites the index; the peer is never told; conversation rows, conv_state, blobs, drafts, avatar pin stay.
VERIFIED hole: handle_query.rs attest merge union-adds every cloud-blob contact not held locally via CloudContact::to_contact → Contact::from_pin stamps roster_updated = now, which outranks the tombstone and re-publishes the contact fleet-wide; boot never re-uploads the blob.
Other holes (agent survey, not yet verified): a failed tombstone push is never retried (current_roster always emits tombstone:false); the AEAD re-seal rebuilds the slot without tombstones; a sibling's fresh roster stamp can outrank it; chain-sync re-adopts chains for a booted friendship.
No conversation-level delete exists anywhere; per-message delete (true-wins tombstone) is the only history removal.
**How to apply:** a nuke needs a local tombstone ledger re-emitted every push, a tombstone-aware cloud merge (+ re-upload on boot), guards in roster merge / cloud merge / chain adopt, and a purge of rows/conv_state/drafts/blobs; a fleet-wide history wipe needs a new conversation-level tombstone (collides with retention.md §5 braid-bound rows). Related: [[project_lifecycle_flows]], [[project_retention_winnow]].
