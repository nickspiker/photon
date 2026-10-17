---
name: project_attachments
description: "attachments SHIPPED (v1 2026-07-26, typed 2026-09-10; docs/attachments.md): row = pointer on the chain, blob over PT, sealed files beside the vault never in it; the attachment IS the message"
metadata:
  type: project
---

Design is docs/attachments.md (canonical). Load-bearing rules:
- Row/blob split: the row rides the ordinary chain send (ACK, fleet sync, history, tombstones work unchanged); the BLOB travels over PT, sealed under history_key (friend) or the fleet key (sibling), stored kete-sealed and content-addressed in `<config>/blobs/` — NEVER in the vault (dual-ring writes + vault-grow fsync freeze the UI).
- Wire key = `attach_wire_key` (the relationship of the two DEVICES, not the conversation). Raw markers never reach a glyph: `display_content()`.
- Kind is sniffed from bytes, never name/wire (receiver re-sniffs, stricter wins); micro thumb on the row, AV1-in-VSF preview blob ≤512 px pushed first, chunked originals (256 KB chunks + manifest, want-bitmap resume, 256 MB cap).
- Delete-for-everyone removes the blob file at all three tombstone sites (discard, not shred — [[feedback_discard_not_shred]]).
- "Open original" renders thru opsin to linear VSF RGB ([[project_viewer_is_opsin]]); loose temp files only in sanctioned modules (attachments.rs).
- THE ATTACHMENT IS THE MESSAGE (Nick 2026-09-12): a row shows only its visual; tap the visual opens it, tap beside it opens the details strip (stats line + pill buttons).

**Why:** no cloud at rest (weakest-link rule); Nick: "PT handles the exchange and manifestus handles the storage."
**Next shape:** [[project_chain_text_and_pointers]] (meta + thumbs move off the row) and [[project_fleet_holds_everything]] (sibling replication).
