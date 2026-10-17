---
name: project-great-cleanup
description: "GREAT CLEANUP phases 1-6 SHIPPED 2026-08-20; standing rules it left: the fleet is the backup (no migration layer), the photon-dir census is absolute, no compat shims, images byte-exact"
metadata:
  type: project
---

Shipped 2026-08-20 and published as one coordinated flag day (clean start: every device wiped and re-attested).
Standing rules it left behind:
- **The fleet is the backup** (Nick 2026-08-20): no local migration/import layer; a fresh vault fills from chain replication + history sync.
- **The census is absolute** (Nick, after two violations): a file in the primary or secondary photon dir that isn't `<device token>.vsf` / log.vsf is DELETED — "no backey uppey, no convertey, no touchey"; nothing is imported; the vault is born empty.
- **ONE device vault:** `open_session_vault(identity_seed, vault_seed, device_secret)` is the open; device scope from first launch, identity scope at attest (a second identity is refused).
- Blobs are vault values at `keyed_hash(name_key(identity_seed), content_hash)` (seed-keyed, because trie keys sit in plaintext leaves).
- Runtime files (photon.lock, control.sock, the wave spool) live in `$XDG_RUNTIME_DIR/photon/`, keyed by data-dir hash.
- `cargo test` can never mint real vaults (per-PID tempdir roots).
- **No compat shims** (@cfebcc4, "nuke the bullshit"): old-format anything is refused, never tolerated ([[feedback_no_compat_text_hatches]]).
- Images send byte-exact; `scripts/lib/artifact-gate.sh` bans foreign image encoders and fences fs::write to an allowlist; formats, codecs and locations are owner decisions.
- Friend keygens hold at the one spawner until every sibling has a presence verdict.

**Why:** every migration path, backup copy or legacy reader is extra attack and bug surface; the fleet already holds the data.
**How to apply:** before adding a migration, import, backup copy or legacy reader, re-read these rules — the answer is no.
