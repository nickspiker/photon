---
name: project_attachments
description: "attachments v1+v2 SHIPPED (e8baa81 + cd3aa3f) — row/blob split (marker on chain + PT blob, no cloud), sealed files beside vault, save/fetch, true-shred, Android any-file picker, image resample card, live PT progress + attach_have"
metadata: 
  node_type: memory
  type: project
  originSessionId: bf3c2e39-d57b-4469-8848-1780b1b5c927
---

Attachments v1 SHIPPED 2026-07-26 @ e8baa81. THE design split: the ROW is an ATTACHMENT_PREFIX content string (blake3 + name + size, types/contact.rs) riding the ordinary chain send — every downstream system (ACK, fleet sync, history pages, tombstones, fleet-forward) inherited with zero codec changes; the BLOB travels separately over PT (attach_blob/attach_req frames in fgtw/protocol.rs), sealed under history_key (friend) or fleet key (sibling), stored as kete-sealed content-addressed files in <config>/blobs/ — NEVER in the vault (dual-ring doubles multi-MB writes; vault-grow fsync freezes the UI — same reason clutch keypairs went memory-only).

**Why:** no cloud at rest (weakest-link rule); PT + manifestus-adjacent files were the user's explicit direction ("PT handles the exchange and manifestus handles the storage").

**How to apply:** blob helpers = storage::blob_store/load/present/delete; wire key chooser = attach_wire_key (relationship of the two DEVICES, not the conversation); display mapping = display_content() in photon_app.rs — raw markers must never reach a glyph. Attachment blobs TRULY SHRED on delete-for-everyone (only row content is braid-bound) — all three tombstone sites delete the blob file. Notes-to-self fetch uses handle_hash as token fallback (sibling wire key ignores tokens). OPEN: Android send UI (generalize the avatar picker to */*), eager sibling blob replication, resend doesn't re-push the blob (fetch covers it). Related: [[project_fleet_unification_v1]], [[project_clutch_completion_rebroadcast]].
