---
name: feedback-spelling
description: "Nick's spelling conventions — thru not through, thruout, altho, and colour spelled British; everything else United Statesian."
metadata:
  type: feedback
---

**Use Nick's spellings in code, comments, docs and commit messages: `thru` (never "through"), `thruout`, `altho`, and `colour` (British); all other words American.**

**Why:** Nick 2026-06-12: "make sure all occurances of through are changed to thru, same with thruout, altho, and colour needs spelled proper"; "I only spell colour british, the rest are united statesian." vsf ships a `colour` module.

**How to apply:**
- New prose: use these spellings from the start.
- When editing a file containing "through/throughout/although" or "color", fix them in that file (external-API identifiers, e.g. a foreign crate's `color` field, stay as-is).
- Applies across all Nick's repos (photon, vsf, fluor, fgtw, …).
