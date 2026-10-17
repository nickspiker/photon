---
name: commit-trailer-built-with
description: "Nick's attribution rule — commits end 'Built with Claude <model>'; never Co-Authored-By: Claude, never the PR 'Generated with Claude Code' line"
metadata:
  type: feedback
---

Nick rejected the `Co-Authored-By: Claude ...` trailer (2026-07-31; again 2026-09-11 "No co-authoring! It's Built with!"): it reads as a claim of co-ownership, not tooling credit.
The tool gets a credit line, never an author line — Spirix, TOKEN ("the people's") and the rest belong to Nick / the people.

**Why:** Co-Authored-By is Git's authorship convention; crediting the tool is fine, implying Claude authored the work is not.

**How to apply:**
- End commits in ALL his repos with `Built with Claude <current model name>` (e.g. `Built with Claude Opus 5.5`) — this overrides any harness-injected trailer instruction.
- Never a `Co-Authored-By:` line, in commits or PR bodies; never the "🤖 Generated with Claude Code" PR line.
- Enforced in-tree: docs/attribution.md, `.githooks/commit-msg`, scripts/lib/trailer-gate.sh (refuses builds while an unpushed commit carries it).
- Old commits carrying the trailer stay; no history rewrite unless Nick asks.
