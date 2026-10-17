---
name: project_contacts_glow_damage
description: "PARKED minor (2026-07-09, unverified since): contacts search textbox focus-glow not fully repainted on focus-off — stale glow lingers, looks un-deselectable"
metadata:
  type: project
---

Contacts (Ready) screen, search textbox with text + focused: the glow is clipped while active, and after deselect the stale glow stays (the deselect LOGIC fires; the repaint is incomplete).
Traced 2026-07-09: fluor's `Textbox::damage_rect` does expand to `glow_bbox` on focus-off, but the background pass (`rasterize_bg`) appears dirty-gated and skips that sub-region, so the old glow pixels are never overwritten.
**How to apply:** fix by making the focus-off frame repaint the glow_bbox including the background under it; touches dirty-gating, so do it as a focused pass with on-device verify. First check whether it still reproduces (scroll-as-memmove and band painting changed this code since).
