---
name: project_chain_text_and_pointers
description: "DOCTRINE (Nick 2026-10-07): the chain carries ONLY text, formatting, links and typed pointers; every attachment is an off-chain wrapped VSF blob; file heads BUILT 2026-10-07 (head is not identity) — docs/attachments.md 'Target shape'"
metadata:
  type: project
---

Nick 2026-10-07: *"I'd keep the chain simply text, text formatting, links, pointers to waves and beams and pigeons and other blobs and any rich formatting should point to blobs beyond simple toka arrangement bytecode… similar to markdown."* Waves too: the conversation loads on text + pointers, pictures and envelopes fill in after, with the band reserved so nothing jumps.

**Why:** a lane photon must arrive whole before the next decrypts and feeds the braid; history pages, chain sync and retention all assume photons are small; files want chunked, resumable, multi-source, content-addressed transport and independent eviction.
**How to apply:** never put file bytes, previews or file metadata into a row. Open decisions are in docs/attachments.md "Target shape". Related: [[project_fleet_holds_everything]], [[feedback_photons_are_messages]].

**FILE HEADS BUILT 2026-10-07:** `types/attach_head.rs` AttachHead; `AttachRef.head` (wire `fhd`); `AttachRef::pointer`.
**HEAD IS NOT IDENTITY** (Nick: "look for stuff that will throw off determinism"): identity bytes feed advance/strands/digest/row key, and an older build drops the unknown field, so the head stays out (`same_identity`; merge adopts a missing head); msg_hp/ACK hash the RAW decrypted payload.
Hydration (`hydrate`, runtime `head_name`) is never persisted/paged/sent; `head_wants` → `drain_head_wants` → `drain_heads_loaded`. The install-time reconcile skips unread-head rows.

**THE WRAPPED BLOB'S IDENTITY (Nick 2026-10-17):** a wrapped attachment carries its own header `hb` — BLAKE3 over the whole wrapped file with only the hb slot zeroed (computed last, after hp and signing) — and THAT is the content hash: row pointers, `attach_req`, vault addressing and the device-removal have-set all key on it, and a fetched file verifies itself. `hp` (all crypto zeroed, computed first) stays as provenance (same content, any signer). Today's raw-bytes `blake3(file)` carried as `hb` in `fh`/`hash` frames is the pre-wrapper form.
**Rollout:** versioned — the wrapper ships with a back-compat reader for the raw form for 12 releases, then the BUILD HALTS: a compile-time assert in the legacy-reader module (`VERSION_MINOR < ship+12`, from CARGO_PKG_VERSION) so the reader cannot outlive its window. Do the wrapper before (or with) the device-removal have-set so it keys on the wrapped hb from day one.
