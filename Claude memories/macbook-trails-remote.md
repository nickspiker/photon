---
name: macbook-trails-remote
description: "This MacBook's clones can trail the other machine by many versions with REWRITTEN history — remote is canonical; verify patch-equivalence then reset --hard, and pull ALL sibling repos together"
metadata: 
  node_type: memory
  type: project
  originSessionId: c5aa3ef0-d46f-4c40-a374-e2549bebaaef
  modified: 2026-08-23T16:41:05.061Z
---

Nick develops photon on more than one machine. On 2026-08-23 this MacBook's photon clone sat at v0.51.201 while origin/main was at v0.61.6 — and the local tip commits existed on the remote under DIFFERENT hashes (same subjects, rewritten/re-landed history), so `git pull` died on divergent branches.

**Why:** the other machine re-landed the same work and pushed far ahead; nothing local was unique (verified: every local commit's subject existed on origin/main; `git cherry` false-positives on rewrites, subject-match is the reliable check).

**How to apply:** on divergence, compare `git log main..origin/main` / reverse, match local-only commit subjects against origin, then `git reset --hard origin/main`. Never merge the two lines. Then fast-forward EVERY sibling path dep together — vsf, fgtw, fluor, tohu, manifestus, kete, rarangi, nunc, chirp — photon HEAD compiles only against current siblings (stale fluor/fgtw gave 64 unresolved-import errors that look like real breakage). `../winit-patched` is a detached vanilla winit checkout with no photon remote — skip it, and leave its Cargo.lock drift (dpi registry→local, windows-sys downgrades) uncommitted.

Related: [[push-after-landing]], [[photon-not-fmt-clean]]
