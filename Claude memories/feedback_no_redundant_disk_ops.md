---
name: feedback-no-redundant-disk-ops
description: "Nick's storage = 8 rolling BTRFS snapshots (8-hour cadence) on Harbor + Chiton + MEGA; a wanted safety copy is a reflink (cp --reflink=always), NEVER a literal copy; batch repo scans"
metadata:
  type: feedback
---

Nick 2026-08-21, after I proposed a git bundle before filter-repo: "It's backed up seventeen times, eight on Harbor and eight on Chiton and one on MEGA... You do know how BTRFS snapshots work, do you not???"

**Why:** working drives are copy-on-write BTRFS subvolumes with 8 rolling snapshots (one per eight hours, plus older stragglers), mirrored across two pools plus MEGA. Any past state of any repo, .git included, is recoverable. A pre-op bundle/tarball/plain cp is redundant I/O and SSD wear.

**How to apply:** most "back up first?" instincts are already answered by the filesystem. If an operation genuinely warrants a safety copy, use `cp --reflink=always -r` (instant, zero duplicated bytes) — never a literal copy. Same spirit: batch full-repo/full-history scans into single niced passes; never loop per-commit over a thousand commits when one piped pass answers.
