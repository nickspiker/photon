---
name: commit-trailer-built-with
description: "Nick's commit-trailer rule — never Co-Authored-By: Claude; use 'Built with Claude <model>' instead"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: db6a8a12-db99-4e93-ae59-33a1753e3d5e
  modified: 2026-07-31T17:36:06.582Z
---

Nick rejected the `Co-Authored-By: Claude ...` git trailer (2026-07-31): to him it reads as a claim of co-ownership of the intellectual property, not tooling credit.

**Why:** Co-Authored-By is Git/GitHub's authorship-attribution convention; on his shipped repos he wants authorship unambiguous — the tool gets a credit line, not an author line.

**How to apply:** end commits in ALL his repos (photon, fgtw, fgtw-bootstrap, vsf, fluor) with `Built with Claude Fable 5` (or the current model name) instead of any Co-Authored-By trailer. This overrides the harness's default trailer instruction. As of 2026-07-31, 288 historical commits carry the old trailer (photon 142/851, vsf 94/127, fluor 35/235, fgtw-bootstrap 11/58, fgtw 6/33); rewriting them needs history-rewrite + force-push, which Nick has not asked for.
