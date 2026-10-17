---
name: project-xchacha-migration
description: "2026-08-18 stack-wide ChaCha20-Poly1305 → XChaCha20-Poly1305 (192-bit nonce) everywhere incl. the chain stream; photon's read-both died at the flag-day clean start, kete + tohu still carry theirs"
metadata:
  type: project
---

Nick 2026-08-18: dislikes the 96-bit nonce's birthday bound and wants one cipher, one nonce size everywhere — "go full meal deal" at 15 peers, before public launch. Every site moved: kete vault `encrypt_bytes`, fgtw `scoped_blob` + `fanout_seal`, avatar seal, tohu capsules, wave packets/spool, and the tagless chain Layer-2 stream (`crypto/chain.rs`, key context `photon.chain.xchacha.v1`). `bootstrap.rs` stays AES-256-GCM (Web Crypto interop with FGTW). Registry: vsf `WRAP_XCHACHA20POLY1305 = b'X'`.

Read-both status (checked 2026-10-17): photon's paths are GONE (chain `decrypt_layers_legacy` removed; an avatar test pins that a 12-byte-nonce blob is refused). STILL PRESENT, marked MIGRATION: kete `decrypt_bytes` legacy 12-byte branch and tohu's TOHUREB1/TOHUSES1 magics — drop them once no pre-2026-08-18 vault or capsule remains (Nick's call).

**Reusable pattern for cipher migrations:** AEAD sites can read-both with no version byte (the tag disambiguates); tagless stream sites need a validator (parse) or a version byte; magic-versioned formats read-both off the magic.
Related: [[feedback_vsf_readers_width_agnostic]].
