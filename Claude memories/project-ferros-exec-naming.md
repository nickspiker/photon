---
name: project-ferros-exec-naming
description: "ferros exec/naming design notes (Nick 2026-08-27, born from the deploy.sh path bugs) — no ambient cwd, bind-don't-search, VSF headers not #!, package roots not $0"
metadata:
  type: project
---

Nick 2026-08-27 ("mental notes for ferros later... because `.` is dumb"), after a night of Unix path wounds (deploy.sh run as ./mnt/... from /, $0-relative source doubling, bridge shell cwd ambushes).
**Root defect:** Unix naming depends on ambient mutable state (cwd, $PATH) and infers intent from string shape (./, #!, +x) — the same string names different things depending on where you stand.

**The ferros rules:**
1. No ambient cwd in the exec API — relative paths are shell display sugar resolved to absolute identity at the prompt; cd is shell-UI state only (photon's bridge locus strip is the prototype).
2. Bind, don't search — bare command names live in an inspectable bind table: petname → blake3(binary). Path = cache location, hash = meaning; spawn verifies the signature; "+x" becomes "signed and bound".
3. No #! — an executable is a VSF-framed object; interpreter/signature/capabilities live in the TYPED header, never sniffed from content.
4. Package root by construction — the loader hands every program its identity + package root; the $0/dirname dance cannot exist.
5. Execution is a verb (run bound-name / identity), never inferred from slash-shape or a file bit.

Related: [[no-private-handles]] (petname model), photon's typed-refs rework (the userland prototype of 3).
