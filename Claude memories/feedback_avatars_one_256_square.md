---
name: feedback-avatars-one-256-square
description: "HARD (owner 2026-11-17): every avatar, photo or default tapuwae mark, is ONE 256x256 square, black past the circle + 3 px; every size is a resample of it with the circle cut at the destination; marks rendered once at mint, kept in the vault as AV1"
metadata:
  type: feedback
---

Owner 2026-11-17, verbatim: "ALL should be rendered 256x256 with the pre-cropped expanded circle to cut down on encoding overhead in the unseen edges, keep it square but just cut it to black with a 3px expand, this applies for ALL, the default brot ones, all that. upon the mint of the attest, that should be rendered and used, can be done locally since it will be identical, no need to recompute and absolutely no need to render it bigger than 256x256. it should always be a resample from the 256x256 to the dest, then the nice pixel perfect crop applied, ring, etc."

**Why:** he was surprised marks were rendered per size ("it ALWAYS WAS 256x256"): b0da7536 (2026-10-29) had drawn marks exactly at each diameter, so a pinch re-ran tapuwae at every step.
**How to apply:** never render an avatar or mark at any size but 256, never at a new size during a zoom; draw = resample the 256 square (`update_avatar_scaled`) then `draw_avatar`'s circle crop at the destination. Marks: `prewarm_marks` on a worker reads the vault copy ("mark" key, AV1 in VSF) or renders once and keeps it (photon 83a6ff92). New photos are encoded unmasked out to r + 3 px, black beyond (079e3fb2). Related: [[project_font_bundle]], [[feedback_render_never_touches_vault]].
