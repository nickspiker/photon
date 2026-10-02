---
name: project_braid_salt_codec
description: CONVICTED+FIXED 2026-10-01 — the Jeff/Nick "chain fork" was the chains-blob codec storing the salt source (previous row's IDENT) as lossy NFC `x` text; a typed row's 0xFF ident came back changed after any vault reload / chain-sync adoption → same key, different salt, every later row garbage on both of Nick's devices; now raw wrapped bytes (`last_ident`, pending `ident`) with a one-release bridge reader; the salt DESIGN stands
metadata:
  type: project
---

Nick 2026-10-01: "salt. Do we need it? … not even sure my original spec was right given real fleet and group dynamics." Investigated against braid.md §3/§6, lanes.md, fleet-sync.md and a code inventory.

**Conviction:** desktop + phone both logged `CHAIN DECRYPT lane=d060a396 key#7f65e063 salt#c2301946` vs Jeff's `CHAIN ENCRYPT … key#7f65e063 salt#af98968b, last_plaintext_len = 11`. Same key, different salt = a different PREVIOUS IDENT. storage/friendship.rs wrote `last_plaintext` (and the pending's `plaintext`) as `VsfType::x(from_utf8_lossy(bytes))`; VSF text is NFC-normalized; a typed row's ident starts 0xFF (`ident_typed`, row_control.rs) → U+FFFD → 11 bytes became 13. Nick's phone reloaded from the vault at 23:05:51 and the desktop adopted the phone's chain-sync blob at 23:05:15 ("caught up chain — sibling was ahead"); Jeff's app did not restart until after he sent, so his copy stayed raw. Jeff's lane rotated (wedge heal) and traffic resumed; one of his rows is unreadable on Nick's side.

**Fix (no wire change, each device fixes itself):** the lane's last ident and the pending's ident ride the chains blob as raw wrapped bytes (`VsfType::v(b'I', …)`), read back exact; the old text fields are read as a bridge for blobs written before (exact for NFC text rows), to be REMOVED once every device has rewritten its blobs. Test `typed_and_non_nfc_idents_survive_the_round_trip`. braid.md §3.3 records it; §3.0's domain string corrected to `PHOTON_SALT_v\x01`; lanes.md checkpoint tuple says "last ident (raw bytes)".

**The design question, answered:** the content-derived salt is NOT the problem. Every legitimate holder of a lane position gets the previous ident exactly: a sibling adopting a checkpoint (it rides the blob), a device after a row's deletion (the blob keeps its own copy; rows keep content), a from-join molecule member (every lane starts at 0). The one holder it does not reach — a from-genesis joiner adopting lanes at position k>0 — lacks the chain state itself (frames gap-buffer forever; molecules §8 D5): a bigger, separate gap, read from code, untested.

**RETIRED LANE RULE BUILT 2026-10-01 (docs/lanes.md "A retired lane is retired, not forked"):** `latest_lane_by_signer` on PhotonApp (in memory, (friendship_id, signer device key) → lane label, written on every clean commit); garbage past verify on any OTHER lane from that signer = straggler, logged, streak untouched. Molecule garbage (`chains.molecule`) never escalates to a pairwise re-key / sibling reset either. Not persisted on purpose: first clean frame after launch seeds it.

**OPEN:** (1) from-genesis molecule join + group weave = a protocol arc, scoped in docs/molecules.md §11 (rows carry lane/author/hashes/weave refs/era as raw bytes → molecule history request/serve → replay lanes → only then drop anchor_only); see [[project-molecules-build]]; (2) remove the `last_plaintext`/`plaintext` bridge readers once blobs are rewritten.
