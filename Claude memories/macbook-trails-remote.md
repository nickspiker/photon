---
name: macbook-trails-remote
description: "A MacBook clone can trail origin by many versions with REWRITTEN history — remote is canonical; verify by subject match, reset --hard, and fast-forward ALL sibling repos together"
metadata:
  type: reference
---

Nick develops on more than one machine. 2026-08-23 the MacBook's photon sat at v0.51.201 vs origin v0.61.6, its tip commits present on origin under DIFFERENT hashes (re-landed history), so `git pull` died on divergence.

**How to apply:** on divergence, list `git log main..origin/main` and the reverse, match local-only commit SUBJECTS against origin (`git cherry` false-positives on rewrites), then `git reset --hard origin/main` — never merge the two lines. Then fast-forward EVERY sibling path dep together (vsf, fgtw, fluor, opsin, tohu, manifestus, kete, rarangi, nunc, chirp…): photon HEAD only compiles against current siblings (stale fluor/fgtw gave 64 unresolved imports that look like real breakage). `../winit-patched` has no photon remote — skip it and leave its Cargo.lock drift uncommitted.
Related: [[push-after-landing]].
