---
name: Keep source map updated
description: When adding/removing pub items or files in Photon, update the source map comment block at the top of src/lib.rs
type: feedback
---

When adding or removing public functions, structs, enums, or files in the Photon codebase, update the source map comment block at the top of `src/lib.rs`.

**Why:** Nick wants a quick-reference architecture tree with function names directly in lib.rs so it's always visible when debugging. Without keeping it current it becomes misleading.

**How to apply:** After any change that adds/removes/renames a pub item or .rs file, check whether the source map block at the top of lib.rs needs updating. Keep the box-drawing formatting consistent.
