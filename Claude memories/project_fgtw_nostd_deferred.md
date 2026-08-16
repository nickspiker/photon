---
name: project_fgtw_nostd_deferred
description: "fgtw crate stays std until ferros — no_std is a ferros-era concern, not a migration constraint"
metadata: 
  node_type: memory
  type: project
  originSessionId: 0b164fd9-062c-407e-b4bb-8f6be8d1982d
---

The `fgtw` crate is **std for now**; `no_std` is deferred until ferros (confirmed by user 2026-07-04: "I don't actually think we need no_std until we get to ferros").

**Why:** the crate's only consumers today — photon and the FGTW worker (`fgtw-bootstrap`, a wasm32 cdylib using the std prelude) — are both std, and the `vsf` codec it rides pulls std in every real build (`crypto` → `rand/std`, `inspect` → `std`). A `#![no_std]` fgtw would sit on a std vsf and produce no embedded binary, so it's `alloc::` friction for zero payoff until a genuinely no_std consumer exists.

**How to apply:** move code into the crate verbatim (std `Vec`/`String`/`format!`/`HashMap`/`OnceLock` are fine — no `alloc::` prefixes, no BTreeMap/spin refactors needed). The `fanout` feature still gates the extra crypto deps (x25519/chacha/rand/voca/num_bigint) off the worker's base surface, keeping the no_std door open for the ferros era. Relates to the fleet-substrate migration ([[project_manifestus_custodes_split]] sibling refactor) and the ferros OS-layer line ([[project_secret_memory_hygiene]]).
