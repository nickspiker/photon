---
name: project-xchacha-migration
description: "2026-08-18 stack-wide ChaCha20-Poly1305 → XChaCha20-Poly1305 (192-bit nonce) everywhere incl. the chain stream; every read-both is gone (photon at the flag day, kete + tohu 2026-10-17)"
metadata:
  type: project
---

Nick 2026-08-18: dislikes the 96-bit nonce's birthday bound and wants one cipher, one nonce size everywhere — "go full meal deal" at 15 peers, before public launch. Every site moved: kete vault `encrypt_bytes`, fgtw `scoped_blob` + `fanout_seal`, avatar seal, tohu capsules, wave packets/spool, and the tagless chain Layer-2 stream (`crypto/chain.rs`, key context `photon.chain.xchacha.v1`). `bootstrap.rs` stays AES-256-GCM (Web Crypto interop with FGTW). Registry: vsf `WRAP_XCHACHA20POLY1305 = b'X'`.

Read-both status: ALL GONE. Photon's at the flag-day clean start (an avatar test pins a 12-byte-nonce blob refused); kete `decrypt_bytes` and tohu's TOHUREB1/TOHUSES1 readers dropped 2026-10-17 (Nick: "Ditch the old! Clean it!") - tests now pin the refusals.

**Reusable pattern for cipher migrations:** AEAD sites can read-both with no version byte (the tag disambiguates); tagless stream sites need a validator (parse) or a version byte; magic-versioned formats read-both off the magic.
Related: [[feedback_vsf_readers_width_agnostic]].
