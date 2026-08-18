---
name: feedback-handles-byte-precise
description: "HARD RULE (Nick, stated MULTIPLE times, violated repeatedly): handles are BYTE-PRECISE - Nick ≠ Nick, whitespace-only is a valid handle; the ONLY rule is non-empty. NO canonicalization, NO trimming, NO case folding, EVER"
metadata:
  type: feedback
---

**Nick's rule, verbatim intent (2026-08-18, after discovering `canonical_handle` had crept in AGAIN):** "Nick is distinct from Nick and '   ' is a literal handle, the ONLY exception to the rule is no empty handles, that's it. encode as vsf text type x, that's it. trailing tabs, spaces, I don't care, that's the human's choice DO NOT TAKE THAT AWAY." He has stated this MULTIPLE times across sessions and it kept being overridden by well-meaning UX reasoning (the "double handle proof" fork incident was used to justify case/space/camelCase folding in `fgtw::keys::canonical_handle` — removed 2026-08-18).

**Why (his design):** the handle IS the secret and the identity — every byte of it belongs to the human. Folding case/spacing deletes entropy from the keyspace and silently aliases distinct identities. A typo minting a fresh identity is the HUMAN'S choice surfaced by the permanence interstitial ("Yes — forever" on a fresh handle), never something the machine should paper over by rewriting their input.

**How to apply:** every derivation (handle proof, identity seed, vault name, log tag, registry key) hashes the RAW typed string. The only validation anywhere is `!handle.is_empty()`. No `.trim()`, no `.to_lowercase()`, no whitespace splitting, no camelCase detection — in ANY repo (photon, fgtw, ihi, toka, worker). If a UX problem tempts normalization again: the answer is a better CONFIRM screen, never touching the bytes. NFC at the VSF x encoder is NOT an exception to this rule — it IS the rule: Nick's original spec was 'NFC normalization and nothing else' (his words 2026-08-18: 'NFC normalization was the ONLY thing I requested'). The case/space/camelCase folding was layered on top of that instruction against it. Pipeline, complete and final: raw typed string → VSF x (NFC + Huffman, full Unicode) → hash. Anything beyond NFC is a violation, not a judgment call.
