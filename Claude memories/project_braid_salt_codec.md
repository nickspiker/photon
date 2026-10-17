---
name: project_braid_salt_codec
description: CONVICTED+FIXED 2026-10-01 — the Jeff/Nick "chain fork" was the chains-blob codec storing the salt source (previous row's ident) as lossy NFC text; now raw bytes; retired-lane rule built; OPEN = remove the bridge readers
metadata:
  type: project
---

**Conviction (2026-10-01):** same key, different salt on both sides = a different PREVIOUS IDENT. storage/friendship.rs wrote `last_plaintext` (and the pending's `plaintext`) as `VsfType::x(from_utf8_lossy(..))`; VSF text is NFC-normalized and a typed row's ident starts 0xFF (`ident_typed`) → U+FFFD, so 11 bytes came back as 13 after any vault reload or chain-sync adoption.
**Fix (no wire change):** the lane's last ident and the pending's ident ride the chains blob as raw wrapped bytes (`VsfType::v(b'I', …)`); test `typed_and_non_nfc_idents_survive_the_round_trip`; braid.md §3.3 records it.

**The salt design stands** (Nick asked "do we need it?"): every legitimate holder of a lane position gets the previous ident exactly. The one gap is a from-genesis molecule joiner adopting lanes at k>0 — it lacks the chain state itself (docs/molecules.md §11, [[project-molecules-build]]).

**Retired-lane rule BUILT 2026-10-01** (docs/lanes.md "A retired lane is retired, not forked"): `latest_lane_by_signer` (in memory, (friendship_id, signer) → lane) — garbage on any OTHER lane from that signer is a straggler, logged, streak untouched; molecule garbage never escalates to a pairwise re-key.

**How to apply:** never store bytes as `x` text in VSF (NFC rewrites them); typed bytes go in `v`.
**OPEN:** remove the `last_plaintext` / `plaintext` bridge readers in storage/friendship.rs once every device has rewritten its blobs.
