---
name: feedback-no-redundant-disk-ops
description: "Nick's machines are BTRFS-snapshotted many times over (Harbor + Chiton + MEGA) — NEVER invent backup steps (bundles, copies, safety tarballs) before destructive git/file ops, and don't hammer the SSD with avoidable full-repo passes"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: d83fbeaf-685c-4da4-8647-7b49de82fd2c
---

Nick (2026-08-21, before the history rewrite): "It's backed up seventeen times, eight on Harbor and eight on Chiton and one on MEGA... You do know how BTRFS snapshots work, do you not???" — after I proposed a git bundle backup before filter-repo.

**Why:** the working drives are copy-on-write BTRFS subvolumes with a deep snapshot rotation, mirrored across two pools plus MEGA. Any past state of any repo — including .git — is recoverable from the filesystem itself. A pre-operation bundle/copy is pure redundant I/O, and the reflex to add one reads as not understanding his storage architecture.

**How to apply:** before a destructive-looking local operation (history rewrite, bulk delete, vault surgery), the snapshot IS the backup — proceed, or at most ask him to confirm a recent snapshot exists. Never create bundles/tarballs/copy-dirs as safety theater. Same spirit as [[feedback-build-dev-script]] (bare cargo build thrashes the machine): batch full-repo/full-history scans into single passes, nice them, and don't loop per-commit over a thousand commits when one piped pass answers the question.
