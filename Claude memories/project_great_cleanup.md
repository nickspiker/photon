---
name: project-great-cleanup
description: "THE GREAT CLEANUP: Phases 1-6 ALL SHIPPED 2026-08-20 (test isolation; ONE device vault + migration; self-is-a-contact; fleet-first rejoin; JPEG excision + artifact gate; LAN add-device restore); Phase 7 = Nick's flag-day publish + field watch"
metadata: 
  node_type: memory
  type: project
  originSessionId: d83fbeaf-685c-4da4-8647-7b49de82fd2c
---

The Great Cleanup, approved 2026-08-20 (plan file: ~/.claude/plans/pure-forging-acorn.md). All unpublished — ONE flag day with the media-wire rework.

**Phase 1 SHIPPED** (kete 1de0463 + photon 3923d53): cargo test can never mint real vaults; per-PID tempdir roots (cross-RUN leak fixed later in kete f18c128 / photon).

**Phase 2 SHIPPED** (tohu 8709480+267e9b6, kete 0608270+f18c128+99f4f89, manifestus de102a6, photon b5c8cf4+9ebcd33+bde859e+bcb276c+85aa85e):
- ONE device vault: `open_session_vault(identity_seed, vault_seed, device_secret)` is THE open (3 sites: attest worker handle_query.rs, launch.rs, driver.rs resume); device scope hash(thing|device) from first launch, identity scope hash(thing|device|person) at attest (set_identity D2-refuses a second identity).
- NO local migration/import layer (Nick 2026-08-20, superseding the earlier in-place-migration build): THE FLEET IS THE BACKUP (17+ live copies — siblings + friends' devices); migrate_legacy_vault/LEGACY_APP/`.vsf.legacy` backups DELETED, a fresh vault fills from chain replication + history sync.
- THE CENSUS IS ABSOLUTE (Nick verbatim, after two violations — blobfold park + file fold-ins both removed): a file in the primary OR secondary photon dir that isn't `<device token>.vsf`/log.vsf is DELETED — "no backey uppey, no convertey, no touchey"; NOTHING is imported (device_binding.vsf/markers/capsule/blobs/ just die; `binding/party` + `flags/*` + `capsule/reboot` vault entries are the live mechanism, worker index backstops the binding); legacy `Photon/` sibling dirs removed WHOLESALE (canonicalize guard for case-insensitive fs); vault born EMPTY (live_addrs()==0 test).
- Blobs = vault values at `keyed_hash(name_key(identity_seed), content_hash)` addresses (possession-oracle fix carries: trie keys are plaintext in leaves, so addresses must be seed-keyed); presence = `read_stored().is_some()` (no decrypt).
- photon.lock + control.sock + call spool → `$XDG_RUNTIME_DIR/photon/` keyed by data-dir hash; dir unification `Photon/`→`photon/` (LEGACY_APP only for migration reads); wipe walks both casings incl. `.legacy`.
- Perf proven (release): 25MB write 133ms; 500MB write 2.6s / read 1.4s verified — after kete put_growing PRE-SIZES the tract (discover-by-failure doubling measured 78.6s).

**Residual Phase 2 nits**: desktop log stays VOLATILE in temp per Nick's 2026-08-01 call (config census = the vault ring alone); fingerprint is machine-scoped so two OS accounts on one box = same device identity (second attest refused — consistent with one-owner-per-device, told Nick 2026-08-20).

**Phase 3 SHIPPED** (photon 7004d6c+8354020): audit found rid registration/serve/merge/live-push already unified (black hole mechanics fixed piecemeal earlier); branch census = remaining gates are doctrine-approved degenerate forms; NEW: honest self ring (contact_conn_tier fold over siblings via row_ring_tier — a dead sync partner shows GREY, the 13-vs-0 class is now visible), storage seam test (save table == serve table for [pid,pid] degenerate addressing), black-hole log text retired. Live two-device walk = Phase 7 field watch.

**Phase 4 SHIPPED** (c65c861): friend keygens hold at the ONE spawner chokepoint until every sibling has a presence VERDICT (pong or 3-timeout); sibling pair-weaves exempt (the chain-replication channel); unprobed siblings re-ping ~6s so a dead-sibling verdict lands ~18s; keygen result on an already-Complete contact discarded.

**Phase 5 SHIPPED** (f081138): resample_to_jpeg + the whole resample card (slider/checkbox/pills/deferred-encode/probe_image_dims) DELETED — images send byte-exact; scripts/lib/artifact-gate.sh in preflight_gates (foreign image ENCODERS banned, fs::write fenced to allowlist = the approval record); AGENT.md Rule -1 = house formats only, formats/codecs/locations are owner decisions.

**Phase 6 SHIPPED** (a755170): LAN add-device restored — join loop broadcasts UDP discovery each poll round; LanPeerDiscovered carries device_pubkey; own-handle beacons feed lan_heard ledger → candidate rows light "nearby" → tap binds thru the existing two-phase green confirm; words = remote fallback. Registry signature check unchanged (LAN squatter can't become tappable). Field verify pending.

**COMPAT EXCISION @cfebcc4 (Nick: "nuke the bullshit")**: ALL six backwards-compat shims deleted — decrypt_layers_legacy + RX read-both (XChaCha only; legacy ChaCha frame = fork), avatar legacy ChaCha branch (test pins REFUSAL), fleet-settings v6 raw-wrapper fallbacks + legacy one-blob lock key (pubkey_set_union is per-key only), migrate_conversation_tables/domains + legacy derivations, migrate_stale_self_row. NO rolling-upgrade support exists: the flag day is a coordinated clean start (every device wipes local + re-attests, FGTW cleared) — old-format anything is refused, not tolerated.

**Phase 7 = Nick's call**: flag-day publish (media wire v3 + storage v2 + cleanup, all since v0.57) as a COORDINATED CLEAN START (no rolling path — compat shims are gone), FGTW hygiene included. Field watch: notes-to-self convergence both ways, config census two-file, re-serve lines, call counters, LAN tap-add. NOTE: photon GitHub repo taken down 2026-08-20 for handle-scrub (test strings/examples leaked real handles — NEVER put name-like strings in code/tests/memories); commits local-only until Nick stands the remote back up.
