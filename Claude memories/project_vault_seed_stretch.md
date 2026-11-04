---
name: project_vault_seed_stretch
description: "CSTV-02/LIB-10 vault half (decided 2026-11-03, NOT built): stretch the vault's value-key root thru the memory-hard proof — blocked on the fact that kete's identity is both the key input and the address scope; plan = separate key root in kete, read-both, sweep, dated pin"
metadata:
  type: project
---

**Finding (2026-11-03):** `vault_seed == identity_seed` (handle_query.rs sets both from the same value) and kete's `set_identity(vault_seed)` feeds TWO things: `derive_addr_key(addr, identity, secret)` (every value's AEAD key) AND `storage.vault_seed()`, which photon uses as the SCOPE of `vault_key(domain, scope)` at ~35 sites (settings, molecules, drafts, peers, booted, chains…). Stretching the seed therefore moves every self-scoped ADDRESS, not only the keys, and `rehome_into` (same addresses, new keys) cannot carry it.

**Plan:**
1. kete: a separate key root — `set_key_root([u8;32])`, used by `derive_addr_key` instead of the identity; default = identity so untouched vaults behave as today. `identity()` keeps serving addresses and file names.
2. photon: at attest, `root = ihi::handle_proof(blake3::derive_key("photon vault root v1", identity_seed))` (~1 s, memory-hard, SECRET output — the public handle proof is over a different input); cache it in the capsule's `vault_seed` slot (that slot is redundant today) and switch the handful of `session.vault_seed` SCOPE uses to `identity_seed`.
3. Conversion: reader tries the new root then the old (`MIGRATION-EXPIRES: 2027-02-01` at least three months out); a sweep re-keys values in place on the librarian; a crash mid-sweep is harmless because reads try both.
4. The identity-seed half (the public key as an offline handle oracle) waits for handle succession ([[project_handle_rename]]).

**Why:** Nick called it "straightforward" from the KDF view; the address coupling is the part that was not visible from the finding. Report, do not build in haste.
