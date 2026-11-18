---
name: feedback_builds_off_tmp
description: "big cargo targets never in /tmp or the scratchpad (RAM tmpfs with a per-user quota); put them on /mnt/Harbor/Code, not /mnt/Harbor/tmp"
metadata:
  node_type: memory
  type: feedback
  originSessionId: 17b90e8e-8b1a-448e-8ac7-22b057e9a0d7
  modified: 2026-11-18T05:52:05.580Z
---

/tmp is a RAM tmpfs with a per-user quota; the session scratchpad lives under it.
On 2026-11-17 a scratch sibling layout's CARGO_TARGET_DIR (12 GB) filled it and other sessions' commands failed with EDQUOT (reported by the ferros coordinator).
**Why:** one build target is gigabytes; the quota is shared by every session on the machine.
**How to apply:** a scratch sibling layout (worktrees + symlinks, as for a fluor branch check) may live in the scratchpad, but its CARGO_TARGET_DIR goes on /mnt/Harbor/Code (never /mnt/Harbor/tmp, which the owner clears); delete the target the moment the check is done. Related: [[feedback-one-build-per-check]].
