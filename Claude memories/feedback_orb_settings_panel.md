---
name: feedback_orb_settings_panel
description: "the orb opens the settings/about/help panel; device management is its own page inside it; back is back, the orb never carries a context action"
metadata:
  type: feedback
---

The orb (chrome app icon) is the entry to the settings/about/help panel — never a direct action.
Device management (add/remove/rename) lives on its own page WITHIN that panel.
"Back is back, the orb is settings" (2026-07-04): the orb never carries cancel/toggle/back — navigation is a dedicated back control.

**Why:** the orb is general-purpose chrome; binding it to one feature doesn't scale as settings arrive.

**How to apply:** route orb → panel → page → flow. Built: Ready → panel with a nine-page nav rail (`on_orb_click`, src/ui/photon_app/input.rs); the interim orb→AddDevice entry moved to the Fleet page's "Add device" pill. Design: docs/settings-panel.md; device flows: docs/device-lifecycle.md.
