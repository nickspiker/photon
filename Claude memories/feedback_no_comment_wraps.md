---
name: feedback-no-comment-wraps
description: "Never hard-wrap comments, doc-comments or markdown prose; one logical line per sentence/paragraph, in EVERY language"
metadata:
  type: feedback
---

In comments, doc-comments (`///`, `//!`) and markdown body text, **never hard-wrap at any column**. One sentence / paragraph = one source line, however long.

**Why (Nick's framing, 2026-08-14):** hard-wrapping chops a coherent idea at RANDOM line-returns mid-thought — where the column ran out, not where the meaning breaks — so the eye must reassemble it. One idea = one line keeps it whole. Line length is irrelevant; the mid-idea chop is the crime. Explain the rule in those terms, never as "the 60-column gate heuristic". He calls repeat offences "the line wrap virus" (3+ flags; 2026-07-09 he deleted whole wrapped files). He LIKES thorough comments — only the wrapping is the problem.

**How to apply:**
- After writing ANY comment: if one sentence spans two or more `///`/`//`/`#` lines, join it onto one line.
- Discrete items (bullets, separate banner sentences, code) stay separate lines; only collapse what was wrapped for display width.
- Language-agnostic: Rust, shell `#`, gradle, TOML, YAML, README/markdown — config files are not exempt.
- The lib.rs source-map block must not wrap either.
Same rule, enforcement details: [[no-wrapped-comments]].
