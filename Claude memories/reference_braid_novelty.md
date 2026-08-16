---
name: reference_braid_novelty
description: "Prior-art / novelty assessment of the braid construction — what's prior art, what's plausibly novel, what to search before claiming originality"
metadata: 
  node_type: memory
  type: reference
  originSessionId: a870fe1a-87a1-4050-9f87-dda20f62c7bc
---

Assessment 2026-06-28 (NOT a real lit/patent search — done from memory, no tooling). The braid (see [[project_chain_advance_desync]], CHAIN.md "The Braid" v0.1) builds on heavy prior art; any originality is in the COMPOSITION, not the primitives.

**Definitely prior art (the substrate):**
- Double Ratchet (Signal, Marlinspike–Perrin 2013–2016) — forward-secret rolling key chains, per-message advance. Assume any reviewer knows it.
- Skipped-message-key buffering for out-of-order (Signal already does this; the braid's gap-buffer + replay + strict-in-order is a different mechanism — re-derive in order vs cache per-msg keys — but "handle reordering in a ratchet" is not a novel goal).
- Hash-chained message ordering (prev_msg_hp) = per-conversation hash-linked log, Merkle/blockchain-adjacent, well-trodden.
- Mixing auxiliary entropy into KDF ratchet steps — generic.
- Memory-hard mixing category (spaghettify is ours, but scrypt/Argon predate the category).

**Plausibly novel (worth examining — composition):**
1. **THE claim to defend:** weaving TWO DISTINCT prior peer plaintexts per step, depth-1 reach over a WINDOW (not the immediately-prior step), where which two is CSPRNG-selected yet EXPLICITLY named on the wire so the receiver never guesses. "Random selection is safe *because* it's referenced explicitly." Not in the obvious canon.
2. High-res physical timestamp (eagle_time, 704ps) as a provably-unique-per-device content reference doing triple duty (ordering / weave-pointer / storage key); collision only adversarial (two fleet devices, same tick).
3. Ingredient = recoverable plaintext-from-DB, not retained key material → no extra forward-secrecy cost ("a messenger keeps your messages anyway").

**Cautions:** "no prior art I know of" ≠ novel (messaging-crypto lit is deep — ePrint, theses, USENIX/CCS). And novel ≠ secure — the reach-back-into-a-window + framing wants formal/peer review before leaning on it.

**Before claiming originality, actually search:** Google Patents + IACR ePrint + Semantic Scholar for "double ratchet out-of-order", "message franking", "causal ordering secure messaging", "KDF auxiliary input ratchet", "randomized re-keying messaging". Get a messaging-crypto person (or patent attorney for the IP angle) to sanity-check claim #1 specifically.
