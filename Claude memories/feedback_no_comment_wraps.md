---
name: feedback-no-comment-wraps
description: "Don't hard-wrap comments or doc-comments at 80/100 chars; one logical line per sentence/paragraph"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 9ca1f039-527e-467a-81cb-1540c007c7b0
---

When writing comments, doc-comments (`///`, `//!`), or markdown body text, **do not hard-wrap at any column**. One sentence / paragraph = one line in the source, however long.

**Why (the user's own framing, 2026-08-14):** it is NOT about "a sentence continuing onto the next line" as some grammatical rule — that is only how the gate DETECTS it. The actual harm is that hard-wrapping chops a coherent idea into RANDOM line-returns mid-thought: the break lands wherever the column ran out, not where the meaning breaks, so the eye has to reassemble the idea instead of just reading it. One idea = one line keeps the idea COHERENT. Line length is irrelevant; the mid-idea chop is the crime. (The user demonstrated by typing a reply hard-wrapped every ~15 chars — "impossible to read" — to make the point visceral.) Do NOT describe this rule back to the user in terms of the 60-column gate heuristic; describe it as keeping the thought whole. Secondary: hard wraps also churn diffs and rustdoc/markdown reflows anyway, so source wrapping buys nothing.

**How to apply:**
- Free text in comments, doc-comments, and markdown: keep it on one line per paragraph.
- *Discrete* items that are intentionally separate lines (bullet lists, separate header sentences in generated-file banners, code) stay as separate lines — only collapse what was wrapped *for display width*.
- Same rule for README.md and other markdown the user maintains.
- **Language-agnostic — this includes shell (`#`), gradle/groovy, TOML, YAML, etc., NOT just Rust.** Slip mode to watch for (caught 2026-06-27): treating shell/config files as "config" and reflexively col-wrapping their `#` comments while correctly not-wrapping Rust `///`. The rule is the same in every language; the user re-flagged it ("the line wrap virus is back") on shell/gradle comments specifically.

**RECURRING — the user calls it "the line wrap virus" and has flagged it 3+ times.** 2026-07-09: violated it across an entire multi-file session (fleet weave + S + device removal) despite having this memory; the user got furious, started DELETING whole `.rs` files that had wrapped comments (calling them infected), and said "I love comments! Just don't fuck them up." The content/verbosity is FINE — the user LIKES thorough comments. The ONLY problem is breaking one sentence across multiple `///`/`//` continuation lines. **Detection heuristic: after writing ANY comment, if a single sentence spans two or more `///` or `//` lines, JOIN it onto one line — however long.** A multi-line comment is only OK when each line is its OWN discrete sentence/bullet, never a display-width wrap. Also flagged same day: the lib.rs source-map box-drawing block (│ ├──) reads as "ascii random jibberish" when it wraps — keep source-map entries as readable non-wrapping lines.
