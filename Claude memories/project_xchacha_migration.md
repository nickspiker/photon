---
name: project-xchacha-migration
description: "2026-08-18 stack-wide cipher migration: ChaCha20-Poly1305 (96-bit nonce) → XChaCha20-Poly1305 (192-bit nonce) EVERYWHERE, incl. the chain stream layer as XChaCha20. Read-both compat kept for a few versions then removed. Nick: full meal deal at 15 peers before public launch."
metadata:
  type: project
---

**Why (Nick, 2026-08-18):** he took a hard look at the spec and disliked the 96-bit nonce (birthday bound on random nonces). Goal was BOTH security (kill the birthday bound) AND spec uniformity (one cipher, one nonce size everywhere). At 15 peers pre-public-launch he said "go full meal deal" — migrate every site including the tagless chain stream cipher. "I'd like to get it prim and proper before I start yelling COME GET YOUR HANDLES WHILE THE IRON'S HOT." Compat read-both for a few versions, remove once all vaults/frames rewritten; "once we have 5 million users it's a different story."

**Migration policy (per site, decided by nonce model):**
- **Random-nonce AEAD sites** (where 96-bit actually had birthday risk): write XChaCha (24-byte nonce), READ-BOTH by length — the Poly1305 tag disambiguates (a wrong-width attempt fails auth at 2^-128), no version byte needed. Lazy upgrade on next write. Sites: `kete::{encrypt_bytes,decrypt_bytes}` (the vault, single choke point — biggest blast radius), fgtw `scoped_blob::{seal_value,seal_content}`, photon `avatar::encrypt_av1_data_with_key`.
- **Fixed-zero-nonce AEAD sites** (key unique per wrap, nonce irrelevant): `[0;12]`→`[0;24]`, read-both by trying both. fgtw `fanout_seal`, `scoped_blob::seal_slot`.
- **Magic-versioned capsules**: bump the 8-byte magic (TOHUREB1→2, TOHUSES1→2), read-both keyed OFF the magic (clean, no tag-guessing). tohu `store_reboot_capsule`/`seal_session`.
- **Counter-nonce, unpublished** (call media, new): switch outright, no read-both. `call/packet.rs` (seq nonce), `call/spool.rs` (counter). XChaCha buys nothing here (counter never repeats) but uniformity.
- **THE HARD ONE — chain Layer-2 stream** (`crypto/chain.rs`, the message braid): raw ChaCha20 STREAM, no Poly1305 tag, so read-both can't validate by tag. Switched to **XChaCha20** stream (24-byte nonce via `derive_xnonce`, key context bumped `photon.chain.chacha.v0`→`photon.chain.xchacha.v1` so keys are disjoint). Read-both via **PARSE-AS-VALIDATOR** in the RX worker (`ui/photon_app/status.rs`): the memory-hard scratch (Layer 3) is cipher-independent + shared; try `decrypt_layers` (XChaCha) → `message_package::parse` → if it parses, use it; else `decrypt_layers_legacy` (ChaCha) → parse → use if it parses; if NEITHER parses it's a genuine fork, hand the current-format bytes down so the fork detector behaves exactly as before. `commit_braid_rx` UNCHANGED (still gets one plaintext). This keeps the just-stabilized (10-round soak) fork detector honest.

**Left ALONE deliberately** (documented as intentional, not oversight): the chain layer got zero security benefit (per-message key rotation + unique-per-message timestamp already precluded nonce reuse) — Nick chose to convert it anyway for uniformity; I flagged the risk first via AskUserQuestion. `bootstrap.rs` uses AES-256-GCM (Web Crypto interop with FGTW), untouched.

**KEY PATTERN for future cipher migrations (reusable):** AEAD sites → read-both is SAFE and needs no version byte because the auth tag is the disambiguator. Tagless stream sites → need a validator (parse) or a wire version byte. Magic-versioned formats → read-both off the magic. See [[feedback-vsf-readers-width-agnostic]] for the sibling doctrine (never variant-match widths).

**Registry:** vsf `crypto_algorithms.rs` — `WRAP_XCHACHA20POLY1305 = b'X'` (new default), `WRAP_CHACHA20POLY1305 = b'c'` (legacy). braid.md §1.3/§5 updated to XChaCha20.

**Repos touched (all committed together):** kete, tohu, fgtw, vsf (siblings) + photon. Each has a legacy-format read-both test. 228 photon tests green, Android cross-clean.

**MIGRATION-EXPIRES (remove read-both a few versions out, once all 15 peers updated + vaults rewritten):** the legacy branches in kete `decrypt_bytes`, tohu `load_reboot_capsule`/`open_session` (drop V1 magics), fgtw scoped_blob/fanout open fns, avatar decrypt, chain `decrypt_layers_legacy` + `derive_nonce` + the RX worker fallback. Grep "MIGRATION" / "legacy" / "read-both". Do NOT remove until confirmed no pre-migration data or peers remain.
