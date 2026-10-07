---
name: project_chain_text_and_pointers
description: "DOCTRINE (Nick 2026-10-07): the chain carries ONLY text, formatting (toka/markdown-like), links and typed pointers; every attachment is an off-chain wrapped VSF blob (meta + thumbnails + byte-exact original); unbuilt, a flag day — docs/attachments.md 'Target shape'"
metadata:
  type: project
---

Nick 2026-10-07: *"strip it all down… store the pdf or tiff or whatever in a wrapped vsf blob with a thumbnail but I'd keep that off chain. I'd keep the chain simply text, text formatting, links, pointers to waves and beams and pigeons and other blobs and any rich formatting should point to blobs beyond simple toka arrangement bytecode… similar to markdown."*

**Why:** a lane photon must arrive whole before the next decrypts and feeds the braid; history pages, chain sync and retention all assume photons are small; files want chunked, resumable, multi-source, content-addressed transport and independent eviction.

**How to apply:** never put file bytes, previews or file metadata into a row. Today the row still carries `AttachMeta`, the sender's filename and the ≤1730-byte micro thumb — those move into the wrapper when this is built. Open decisions (instant picture, what the pointer names, embedded camera thumbnails, flag-day timing) are listed in docs/attachments.md "Target shape". Related: [[project_fleet_holds_everything]], [[feedback_photons_are_messages]].

**Waves too (Nick 2026-10-07):** "I'd do the same for waves so the convo loads fast and then the thumbnails and content itself dynamically loads after the convo is already up. otherwise the user is waiting 1/2s for large attachments." The wave row's inline envelope moves into the recording's wrapper; a conversation opens on text + pointers alone and every picture/envelope fills in afterwards, with the band reserved so nothing jumps.
