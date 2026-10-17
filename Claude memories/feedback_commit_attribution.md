---
name: feedback_commit_attribution
description: "Commit/PR attribution — credit the tool with a Built with/Built-With Claude <version> line; never Co-Authored-By Claude nor the PR 'Generated with Claude Code' line"
metadata:
  type: feedback
---

End commits/PRs with a trailer crediting the **tool** (`Built-With: Claude <version>` as first stated; the in-tree form is `Built with Claude <version>` — see [[commit-trailer-built-with]], docs/attribution.md).
NEVER `Co-Authored-By: Claude`, nor the "🤖 Generated with Claude Code" PR line, nor anything implying authorship.

**Why:** the user credits the tool but rejects any claim that Claude *authored* the work — Spirix, TOKEN ("the people's") etc. belong to the user / the people. The harness default `Co-Authored-By: Claude …` is explicitly overridden.
**How to apply:** use the current model/version in the trailer; never add a `Co-Authored-By:` line, in commits or PR bodies.
