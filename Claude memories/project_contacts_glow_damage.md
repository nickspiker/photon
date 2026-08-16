---
name: project_contacts_glow_damage
description: "Contacts-screen textbox focus-glow isn't fully repainted on focus change — stale glow lingers (looks un-deselectable), clips, \"full stack not drawn top-down\""
metadata: 
  node_type: memory
  type: project
  originSessionId: cf9fbed2-aa87-4ebe-b7cf-fca16d308f75
---

Contacts (Ready) screen, search textbox with text + focused: three symptoms that are one root cause — the focus-glow region isn't fully repainted on the frame focus changes.
1. The dirty/damage rectangle clips into the glow when active (glow partially drawn).
2. "Doesn't draw the full stack top-down — missing the glow."
3. Can't deselect the textbox when it has text (it stays looking focused).

Root cause (traced 2026-07-09, NOT from that session's hit-map work — focus/glow/damage untouched): `change_focus()` doesn't force a full-viewport repaint; it relies on the per-widget damage rect to clear the old glow. Fluor's `Textbox::damage_rect` DOES expand to `glow_bbox` on a focus-off transition (verified), but that region only clears if the background pass (`rasterize_bg`) actually repaints under it. The bg pass appears dirty-gated and skips the sub-region, so the stale glow pixels are never overwritten → looks un-deselected (the deselect LOGIC fires — `change_focus(None)` on a HIT_NONE/contact-row click — it's the repaint that's incomplete). Only shows "with text" because the glow is only prominent then.

Fix direction: make the focus-off frame repaint the full glow_bbox including the background under it — either promote a focus change to a scene/bg-dirty over the glow region, or ensure rasterize_bg repaints the damaged sub-region rather than early-returning. Touches the dirty-gating — regression-prone, do it as a focused pass with on-device verify. Pre-existing + minor; parked 2026-07-09.
