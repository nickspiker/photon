---
name: commit-and-push-always
description: "Every finished change is committed and pushed without asking (Nick 2026-11-06: \"commit and push, always\"); include every modified file unless Nick excludes one"
metadata:
  node_type: memory
  type: feedback
  originSessionId: 17b90e8e-8b1a-448e-8ac7-22b057e9a0d7
  modified: 2026-11-06T15:36:47.091Z
---

Nick, 2026-11-06: "commit and push, always." Finished work is committed and pushed as it lands — photon and any sibling touched — never left in the tree waiting for a request.

**Why:** uncommitted edits sat in the tree across a long session (README, a presence fix, fixtures) until Nick had to ask; he expects the tree clean and the remote current every time.

**How to apply:** when a change is done and its tests pass, commit and push straight away, one commit per logical change, staged by explicit path ([[feedback_stage_only_own_files]]), trailer "Built with Claude <model>" ([[commit-trailer-built-with]]). Include every modified file unless Nick explicitly says to leave one out — don't cherry-pick on an assumption that something is in progress. Deploys and releases are still Nick's call ([[nick-publishes]]).
