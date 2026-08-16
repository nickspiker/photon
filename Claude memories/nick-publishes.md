---
name: nick-publishes
description: Nick publishes builds himself — Claude commits/pushes code but never runs the publish scripts; checks only when needed
metadata: 
  node_type: memory
  type: feedback
  originSessionId: db6a8a12-db99-4e93-ae59-33a1753e3d5e
  modified: 2026-08-02T14:38:01.463Z
---

Nick ended the publish-every-batch cadence (2026-08-01): "don't publish when you do code changes, only check if you absolutely need to. I'll publish a few things at once here and there and I'll do it in the shell."

**Why:** publishing per-change burns time and version numbers; he batches several changes into one publish from his own shell while the session keeps working.

**How to apply:** after code changes, commit + push only. Never run scripts/publish/*.sh or dev.sh-for-publish unless he explicitly asks IN THAT MESSAGE — an earlier ask does not carry forward ("I'll run publish. that was a one time thing", 2026-08-02, after I treated one authorized publish as standing). Even for urgent hotfixes: commit, push, tell him it's ready to publish. Run cargo check/test when the change warrants verification, not ritually after every edit. This supersedes the earlier "publish android + mac after every fix batch" instruction from 2026-07-31.

Gate invocation gotcha (2026-08-02): `bash scripts/lib/<g>-gate.sh` only DEFINES the gate function — gates must be run as `bash -c "source scripts/lib/<g>-gate.sh && <g>_gate"` or via desktop.sh, or they pass vacuously.
