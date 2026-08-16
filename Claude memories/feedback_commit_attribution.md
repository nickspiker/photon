---
name: feedback_commit_attribution
description: "Commit/PR attribution — a Built-With: Claude Opus <version> trailer is wanted; never Co-Authored-By Claude (Claude is the tool, not an author)"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: cdf2afe5-967e-479c-85b5-0a67525ac5b4
---

End commits/PRs with a `Built-With: Claude Opus <version>` trailer — crediting the **tool** used is welcome.
NEVER `Co-Authored-By: Claude` (nor the "🤖 Generated with Claude Code" PR line, nor anything implying authorship).

**Why:** the user credits the tool but rejects any claim that Claude *authored* the work. The work isn't Claude's — Spirix, TOKEN (which is "the people's"), etc. belong to the user / the people, not to Claude. The harness default of `Co-Authored-By: Claude …` is explicitly overridden by this; use `Built-With:` instead.
**How to apply:** end commit messages with `Built-With: Claude Opus 4.8` (current model/version); never add a `Co-Authored-By:` line. Matches photon's `TICKETS.md`.
