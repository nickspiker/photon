---
name: reference_braid_novelty
description: "Prior-art / novelty notes for the braid and the zero-bitmap spool — what's prior art, what's plausibly novel (composition only), what to search before claiming originality"
metadata:
  type: reference
---

Recall-based assessments (2026-06-28, 2026-09-17), NOT literature searches.

**Braid** (docs/braid.md). Prior art: Double Ratchet, skipped-key buffering for out-of-order, hash-chained message logs, auxiliary entropy in KDF steps, memory-hard mixing.
Plausibly novel (composition):
1. THE claim: each step weaves TWO distinct prior peer plaintexts from a WINDOW, CSPRNG-selected yet explicitly named on the wire ("random selection is safe because it's referenced explicitly").
2. eagle_time as a provably-unique-per-device content reference doing triple duty (ordering / weave pointer / storage key).
3. Ingredient = recoverable plaintext already in the DB, so no extra forward-secrecy cost.

**Zero-bitmap spool** (`src/storage/spool.rs`, proof in the module doc): pre-zero a file, write wire-SEALED chunks at slot offsets; a 256-bit window is all-zero iff unwritten (p = 2^-256 per window). The file IS the resume bitmap — no sidecar, torn "bitmap says held, data missing" unrepresentable. BitTorrent needs explicit bitfields because its pieces are plaintext (zero runs legitimate); using ciphertext entropy as the SOLE crash-atomic tracking state is the composition.

**Before claiming originality:** search Google Patents, IACR ePrint, Semantic Scholar ("double ratchet out-of-order", "causal ordering secure messaging", "KDF auxiliary input ratchet"), and get a messaging-crypto reviewer on claim 1. Novel ≠ secure.
