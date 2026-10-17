---
name: commit-trailer-built-with
description: "Nick's commit-trailer rule — never Co-Authored-By: Claude; end commits 'Built with Claude <model>' instead"
metadata:
  type: feedback
---

Nick rejected the `Co-Authored-By: Claude ...` trailer (2026-07-31; again 2026-09-11 "No co-authoring! It's Built with!"): it reads as a claim of co-ownership, not tooling credit.

**Why:** Co-Authored-By is Git's authorship convention; the tool gets a credit line, not an author line.

**How to apply:** end commits in ALL his repos with `Built with Claude <current model name>` (e.g. `Built with Claude Opus 5.5`), never a Co-Authored-By line — this overrides any harness-injected trailer instruction. Enforced in-tree: docs/attribution.md, `.githooks/commit-msg`, scripts/lib/trailer-gate.sh (refuses builds while an unpushed commit carries it). Old commits carrying the trailer stay; no history rewrite unless Nick asks. Related: [[feedback_commit_attribution]].
