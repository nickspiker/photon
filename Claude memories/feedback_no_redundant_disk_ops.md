---
name: feedback-no-redundant-disk-ops
description: "Nick's storage: 8 rolling BTRFS snapshots at 8-hour cadence (+ stragglers), Harbor + Chiton + MEGA — a wanted safety copy is a BTRFS REFLINK (cp --reflink=always), NEVER a literal byte copy (bundle/tarball/plain cp = SSD wear for zero reason)"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: d83fbeaf-685c-4da4-8647-7b49de82fd2c
---

Nick (2026-08-21, before the history rewrite): "It's backed up seventeen times, eight on Harbor and eight on Chiton and one on MEGA... You do know how BTRFS snapshots work, do you not???" — after I proposed a git bundle backup before filter-repo.

**Why:** the working drives are copy-on-write BTRFS subvolumes with a deep snapshot rotation, mirrored across two pools plus MEGA. Any past state of any repo — including .git — is recoverable from the filesystem itself. A pre-operation bundle/copy is pure redundant I/O, and the reflex to add one reads as not understanding his storage architecture.

**How to apply (Nick's refinement, 2026-08-21):** the baseline is 8 rolling snapshots, one per eight hours, plus older ones that hang around — so most "should I back up first?" instincts are already answered by the filesystem. If an operation is genuinely dangerous and a dedicated safety copy is warranted, take a BTRFS reflink (`cp --reflink=always -r`) — copy-on-write, instant, zero duplicated bytes. NEVER a literal copy (git bundle, tarball, plain cp): pointless SSD wear. Same spirit as [[feedback-build-dev-script]] (bare cargo build thrashes the machine): batch full-repo/full-history scans into single passes, nice them, and don't loop per-commit over a thousand commits when one piped pass answers the question.
