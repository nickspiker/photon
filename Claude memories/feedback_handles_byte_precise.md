---
name: feedback-handles-byte-precise
description: "HARD RULE (stated MULTIPLE times, violated repeatedly): handles — and ALL human text — are BYTE-PRECISE; the ONLY handle rule is non-empty; no trimming, case folding or canonicalization, EVER"
metadata:
  type: feedback
---

Nick 2026-08-18 (after `canonical_handle` crept in AGAIN): "Kea is distinct from Nick and '   ' is a literal handle, the ONLY exception to the rule is no empty handles, that's it. … trailing tabs, spaces, I don't care, that's the human's choice DO NOT TAKE THAT AWAY."

**Why:** the handle IS the secret and the identity — every byte belongs to the human. Folding deletes keyspace entropy and silently aliases identities; in the field, fold-era chains squatted folded slots and locked out mixed-case people. A typo minting a fresh identity is the human's choice, surfaced by the permanence confirm ("Yes — forever"), never papered over by rewriting input.

**How to apply:**
- Pipeline, complete and final: raw typed string → VSF `x` (NFC + Huffman, full Unicode) → hash. NFC at the vsf x encoder IS the rule (Nick's original spec: "NFC normalization and nothing else"); anything beyond is a violation.
- Every derivation (handle proof, identity seed, vault name, log tag, registry key) hashes the RAW string; the only validation anywhere is `!handle.is_empty()`. No `.trim()`, `.to_lowercase()`, whitespace splitting or camelCase detection — in ANY repo (photon, fgtw, ihi, toka, worker).
- A UX problem tempting normalization gets a better CONFIRM screen, never touched bytes.
- Machine-gated: byte-for-byte KATs on `to_identity_seed` (case / trailing space / tab / newline / NFC composed==decomposed) fail on any transform or encoder drift. On a KAT failure: revert the change, never update the constants.
- WIDENED 2026-08-20 to ALL human text ("Every single text entry point should NEVER be fucked with EVER"): messages, profile values + labels, everything typed or pasted, every widget and platform. No control-char filters, no preview flattening (truncate only), paste verbatim. Keys: Enter=send / Shift+Enter=newline (desktop), Tab/Shift+Tab=focus, Ctrl+Tab=literal tab.
- The ONLY sanctioned transforms: (a) NFC in the vsf x encoder, (b) contact-search case fold (MATCH-only, bytes untouched), (c) pairing-word canonicalisation (system-minted tokens, not human identity).
