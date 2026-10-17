---
name: nick-publishes
description: Nick INITIATES a publish/deploy; when he asks, Claude runs it — and the push that goes with it carries the build notes (RELEASE_NOTES.md → the Updates panel + the website); never publish unasked
metadata:
  type: feedback
---

Nick ended the publish-every-batch cadence (2026-08-01): "don't publish when you do code changes … I'll publish a few things at once here and there." Corrected 2026-10-01 ("that's not quite right"): Nick INITIATES; when he says "run a full deploy.sh" (or a publish), Claude runs it.

**Why:** per-change publishes burn time and version numbers; he batches.

**How to apply:**
- After code changes: commit + push only. Never run scripts/publish/*.sh, deploy.sh or a publish build unless he asks IN THAT MESSAGE — an earlier ask does not carry forward ("that was a one time thing", 2026-08-02). Urgent hotfix: commit, push, tell him it's ready.
- When asked: commit first (explicit paths, [[feedback_stage_only_own_files]]; deploy.sh refuses a dirty tree), one full cargo test if tests changed ([[feedback_test_discipline]]), update RELEASE_NOTES.md so the Updates page and website carry the notes ([[project_update_flow]]), then deploy and push.
- Gate gotcha: `bash scripts/lib/<g>-gate.sh` only DEFINES the function — run `bash -c "source scripts/lib/<g>-gate.sh && <g>_gate"` or via desktop.sh, or it passes vacuously.
