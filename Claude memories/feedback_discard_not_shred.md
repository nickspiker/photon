---
name: feedback-discard-not-shred
description: "HARD (Nick 2026-09-27): no \"shred\"/\"nuke\"/\"truly erase\" claims until we own the flash — deletion = mark for discard; a distinct vault object/file → tell the OS to delete it, otherwise marking the space free is plenty; everything is encrypted at rest anyway"
metadata:
  node_type: memory
  type: feedback
  originSessionId: 87fd33b1-f610-46f9-b464-4fe0dd1e3280
  modified: 2026-09-28T03:54:00.963Z
---

Nick 2026-09-27: "nuke and shred aren't really a thing until we own the flash. we can mark it for discard but that's the best we got, and if it's a distinct vault tell the OS to delete it, otherwise marking free is plenty fine. everything is encrypted at rest anyway."
**Why:** flash wear-levelling and the OS decide when bytes physically go; claiming erasure we can't guarantee is dishonest, and encryption at rest already covers the residue.
**How to apply:** design and describe deletion as DISCARD: drop the text/record from the vault index and mark its space free; for a distinct file or vault object, ask the OS to delete it. Never promise shredding, secure erase or overwrite passes in code comments, docs, UI or release notes. Braid v2's S-per-row ([[project_braid_v2]]) is what lets a row's text be discarded at all; the contact "nuke" ([[project_contact_nuke_gaps]]) means discard + sticky tombstones, not erasure. Revisit only once we own the storage stack (ferros).
