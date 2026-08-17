---
name: reference_ihi_primitives
description: "ihi crate has TWO distinct one-way primitives — the lossy OWF (chaos_amp/spaghettify) vs the memory-hard PoW (handle_proof). Don't conflate."
metadata: 
  node_type: memory
  type: reference
  originSessionId: c2e42b61-550d-4281-b408-7ec369680f6f
---

The `ihi` crate (/mnt/Harbor/Code/ihi/src) implements TWO separate one-way primitives from the TOKEN patent. They are different functions with different jobs — do NOT conflate them (I did, twice, in one session).

**1. The provably-lossy OWF — `chaos_amp` (`chaos_amp.rs`), wrapped by `spaghettify` (`spaghettify.rs`).**
This is the patent's `lossy`/`lossymech` claim, BUILT and bit-exact with PIPE silicon (`/mnt/Harbor/Code/pipe/rtl/chaos_amp_v2.v`). 16 buckets × 24 bits, 16 rounds × 2 phases. Phase 1 = **data-dependent op selection**: `op_idx = val[4:0]` picks from a **32-op menu** (the data being operated on selects the operation — NOT memory access). Menu has 11 lossy + 3 extreme-lossy ops (POPCNT, SAT_ADD, SAT_SUB, PCNT_REPLACE → ≤8 distinct outputs). ~44% of op-applications destroy bits; ~700–2500 cumulative bits destroyed per call, destruction happening WITHIN rounds and compounding. ~10^482 op-selection paths (>> atoms^2). `spaghettify` = BLAKE3-XOF absorb → chaos_amp → smear_hash finalize (BLAKE3⊕SHA3-256⊕SHA-512 defense-in-depth). Preimage-resistant by construction; this is the function whose **lossiness makes outputs mutually unlinkable** (the device-count-privacy property for [[project_keyring_design]]).

**2. The memory-hard PoW — `handle_proof` (`handle.rs`).**
This is the patent's *anti-squatting cost* primitive (the "~1 second, tens of MB, sequential" derivation), BUILT but DIFFERENT. 17 rounds over a ~25MB scratch buffer: Phase 1 sequential hash chain (non-seekable), Phase 2 data-dependent random **memory reads** (cache-hostile, ASIC-resistant), BLAKE3 core. Here the data-dependency governs **which memory block is read** — exactly what the patent's lossy claim distinguishes ITSELF from. Job = make first-claim of a handle expensive in bulk (rations squatting), NOT lossy binding.

**Pipeline:** `handle_to_hash(handle)` = `BLAKE3(VsfType::x(handle).flatten())` (cheap pre-hash, sub-µs, the seed used everywhere — contact keys, avatar keypair seeds, vault). `handle_to_proof(handle)` = handle_to_hash → `handle_proof` (the expensive PoW → public id). So "seed → proof" = the memory-hard step; the lossy `spaghettify` is the separate binding/derivation primitive used for device bindings and the legacy-password OWF.

Other ihi files: `smear.rs` (smear_hash = BLAKE3⊕SHA3⊕SHA512 multi-algo), `lib.rs`.
