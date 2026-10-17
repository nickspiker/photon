---
name: feedback-no-comment-wraps
description: "Never hard-wrap comments, doc-comments or markdown prose in ANY of Nick's repos; one line per sentence/thought, at write time; comment-gate.sh enforces at build"
metadata:
  type: feedback
---

In comments, doc-comments (`///`, `//!`) and markdown body text, **never hard-wrap at any column**, in any of Nick's repos (photon, fgtw, vsf, fluor, …). One sentence / thought = one source line, however long; the editor soft-wraps.

**Why (Nick's framing, 2026-08-14):** hard-wrapping chops a coherent idea at RANDOM line-returns mid-thought — where the column ran out, not where the meaning breaks — so the eye must reassemble it. Line length is irrelevant; the mid-idea chop is the crime. Explain the rule in those terms, never as "the 60-column gate heuristic". He calls repeat offences "the line wrap virus" (called out 4+ times; 2026-07-09 he deleted whole wrapped files). He LIKES thorough comments — only the wrapping is the problem. Writing wrapped prose and de-wrapping later is the failure mode: the rule applies AT WRITE TIME.

**How to apply:**
- After writing ANY comment: if one sentence spans two or more `///`/`//`/`#` lines, join it onto one line.
- Discrete items (bullets, separate banner sentences, code) stay separate lines; only collapse what was wrapped for display width.
- Language-agnostic: Rust, shell `#`, gradle, TOML, YAML, README/markdown — config files are not exempt; the lib.rs source-map block must not wrap either.
- Enforced by scripts/lib/comment-gate.sh (run by dev.sh via desktop.sh; photon src + the fgtw path dep): fails the build on a >60-col comment line ending in a word character whose next same-marker line starts lowercase.
- fgtw-bootstrap is legacy-wrapped: don't churn it, don't add new wraps.
