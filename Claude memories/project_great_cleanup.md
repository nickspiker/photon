---
name: project-great-cleanup
description: "THE GREAT CLEANUP (7-phase approved plan, plans/pure-forging-acorn.md): Phase 1+2 SHIPPED 2026-08-20 (test isolation; ONE device vault + in-place migration + sprawl/blob absorption + runtime-dir artifacts + dir unification + census test + 500MB probe); Phase 3 self-is-a-contact NEXT; then fleet-first rejoin, JPEG excision, LAN-add restore, flag-day publish"
metadata: 
  node_type: memory
  type: project
  originSessionId: d83fbeaf-685c-4da4-8647-7b49de82fd2c
---

The Great Cleanup, approved 2026-08-20 (plan file: ~/.claude/plans/pure-forging-acorn.md). All unpublished — ONE flag day with the media-wire rework.

**Phase 1 SHIPPED** (kete 1de0463 + photon 3923d53): cargo test can never mint real vaults; per-PID tempdir roots (cross-RUN leak fixed later in kete f18c128 / photon).

**Phase 2 SHIPPED** (tohu 8709480+267e9b6, kete 0608270+f18c128+99f4f89, manifestus de102a6, photon b5c8cf4+9ebcd33+bde859e+bcb276c+85aa85e):
- ONE device vault: `open_session_vault(identity_seed, vault_seed, device_secret)` is THE open (3 sites: attest worker handle_query.rs, launch.rs, driver.rs resume); device scope hash(thing|device) from first launch, identity scope hash(thing|device|person) at attest (set_identity D2-refuses a second identity).
- In-place migration: manifestus `live_keys()` trie enumeration → kete `adopt_all_entries_from` raw ciphertext copy (identity-scope KDF unchanged ⇒ verbatim bytes decrypt); legacy rings renamed `.vsf.legacy` backups in old `Photon/` dir (reaped by a later version, wipe takes them).
- Sprawl absorbed as device-scope entries: `binding/party`, `flags/{unattended_reboot,remote_terminal,background_optout}`, `capsule/reboot` (tohu grew seal/open BYTE APIs); settings.vsf DELETED (dead knobs — vsf runtime elision API is gone).
- Blobs = vault values at `keyed_hash(name_key(identity_seed), content_hash)` addresses (possession-oracle fix carries: trie keys are plaintext in leaves, so addresses must be seed-keyed); presence = `read_stored().is_some()` (no decrypt); blobs/ dir folds in at session open (decrypt legacy file → re-hash plaintext → write_addr).
- photon.lock + control.sock + call spool → `$XDG_RUNTIME_DIR/photon/` keyed by data-dir hash; dir unification `Photon/`→`photon/` (LEGACY_APP only for migration reads); wipe walks both casings incl. `.legacy`.
- Perf proven (release): 25MB write 133ms; 500MB write 2.6s / read 1.4s verified — after kete put_growing PRE-SIZES the tract (discover-by-failure doubling measured 78.6s).

**Residual Phase 2 nits**: desktop log stays VOLATILE in temp per Nick's 2026-08-01 call (config census = the vault ring alone); fingerprint is machine-scoped so two OS accounts on one box = same device identity (second attest refused — consistent with one-owner-per-device, told Nick 2026-08-20).

**NEXT: Phase 3** self is a contact ([[feedback-self-is-a-contact]]): fleet-route rid registration in hist_rid_map (black hole at conversation.rs drain_history_pages), live-push trace, ring tier from own-device presence, delete the 17 has_remote branches. Then Phase 4 fleet-first rejoin (gate spawn_next_pending_keygen on unprobed siblings), Phase 5 JPEG excision + artifact gate, Phase 6 add-device LAN discovery archaeology, Phase 7 flag-day publish (ZERO data loss).
