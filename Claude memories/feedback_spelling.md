---
name: feedback-spelling
description: "Nick's spelling conventions — thru not through, thruout, altho, and colour spelled British; everything else United Statesian."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 5f35c813-09a3-4839-8124-5f81ac93298a
---

**Use Nick's spellings in code, comments, docs, and commit messages: `thru` (never "through"), `thruout`, `altho`, and `colour` (British); all other words American.**

**Why:** Stated 2026-06-12 while screening custodes against AGENT.md ("make sure all occurances of through are changed to thru, same with thruout, altho, and colour needs spelled proper"). Earlier (custodes naming session): "I only spell colour british, the rest are united statesian." vsf already ships a `colour` module.

**How to apply:**
- New prose I write: use these spellings from the start.
- When editing a file that contains "through/throughout/although" or "color", fix occurrences in that file (identifiers from external APIs stay as-is — e.g. a foreign crate's `color` field).
- Applies across Nick's repos (photon, custodes, vsf, fluor, etc.), same scope as [[feedback-no-ai-attribution]].
