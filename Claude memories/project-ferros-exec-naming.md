---
name: project-ferros-exec-naming
description: "ferros exec/naming design notes (Nick 2026-08-27, born from the deploy.sh path bugs) — no ambient cwd, bind-don't-search, VSF headers not #!, package roots not $0"
metadata: 
  node_type: memory
  type: project
  originSessionId: 2b62e116-08df-4b87-8444-016a23486f1b
  modified: 2026-08-27T15:30:55.437Z
---

Nick's ask 2026-08-27 ("mental notes for ferros later... because `.` is dumb"), after a night of Unix path wounds (deploy.sh invoked as ./mnt/... from /, $0-relative source doubling, bridge shell cwd ambushes).

**The root defect being designed away:** Unix naming depends on ambient mutable state (cwd, $PATH) and infers intent from string shape (./ prefix, #! sniffing, +x bit). Same string = different referent depending on where you stand.

**The ferros rules:**
1. No ambient cwd in the exec API — relative paths are SHELL DISPLAY SUGAR resolved to absolute identity at the prompt; programs receive resolved identities + explicit location arguments. cd is shell-UI state only (photon's bridge locus strip is the prototype).
2. Bind, don't search — bare command names live in an explicit inspectable bind table: petname → blake3(binary) (the handle/petname model). Path = cache location; hash = meaning. Spawn verifies the Ed25519 signature; the +x bit is replaced by "signed and bound".
3. No #! — an executable is a VSF-framed object; interpreter/signature/required-capabilities live in the TYPED header, never sniffed from content (same content-is-sacred doctrine as photon's typed refs).
4. Package root by construction — the loader hands every program its identity + package root as first-class values; intra-package references are package-relative by API. The $0/dirname reconstruction dance (and its whole bug class) cannot exist.
5. Execution is a verb (run bound-name / identity), never inferred from slash-shape or a file bit.

Relates to [[feedback-vsf-readers-width-agnostic]] (one canonical encoding), [[no-private-handles]] (petname model), photon's bridge locus strip + typed-refs rework (the userland prototypes of 1 and 3).
