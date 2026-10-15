---
name: feedback_no_python
description: HARD — no Python in the photon repo unless proven necessary; build-time artifacts (fonts) are committed, generators live elsewhere or in git history
metadata:
  type: feedback
---

No Python in the photon repo unless it is proven necessary — not as a tool, not in a build script (Nick 2026-10-14, on tools/oxanium-dozenal.py: "I don't want a python dependency… no python allowed in this repo unless proven necessary").

**Why:** a Python/fontTools dependency is a build-environment requirement the project does not want; repo tooling is Rust (tools/*.rs, rustc-built).

**How to apply:** one-off asset work (font merges, glyph synthesis) may use Python in the scratchpad, but the RESULT is committed as the artifact (e.g. assets/Oxanium/Oxanium-VariableFont_wght+glyphs.ttf) and its provenance is written in the loading code's comment. A repeatable generator, if ever needed, is written in Rust. Related: [[feedback_build_dev_script]].
