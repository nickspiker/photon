---
name: reference_ihi_primitives
description: "ihi crate has TWO distinct one-way primitives — the lossy OWF (chaos_amp/spaghettify) vs the memory-hard PoW (handle_proof). Don't conflate."
metadata:
  type: reference
---

The `ihi` crate (/mnt/Harbor/Code/ihi/src) implements two separate TOKEN-patent primitives. Do NOT conflate them (done twice in one session).

**1. Provably-lossy OWF — `chaos_amp` (`chaos_amp.rs`), wrapped by `spaghettify` (`spaghettify.rs`).** The patent's lossy claim, bit-exact with PIPE silicon (`pipe/rtl/chaos_amp_v2.v`). Data-dependent OPERATION selection from a 32-op menu, many ops destroy bits within rounds. `spaghettify` = BLAKE3-XOF absorb → chaos_amp → `smear_hash` (BLAKE3⊕SHA3-256⊕SHA-512). Its lossiness makes outputs mutually unlinkable; used for device bindings and derivations.

**2. Memory-hard PoW — `handle_proof` (`handle.rs`).** Anti-squatting cost: ~1 s, ~25 MB scratch, sequential chain + data-dependent MEMORY reads. Makes bulk handle claims expensive; not lossy binding.

**Pipeline:** `handle_to_hash(handle)` = BLAKE3(VSF-x(handle)) — cheap seed used everywhere. `handle_to_proof(handle)` = handle_to_hash → `handle_proof` → the public id.
Related: [[project_session_registers]], [[project_token_terminology]].
