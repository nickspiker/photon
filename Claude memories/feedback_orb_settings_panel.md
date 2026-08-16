---
name: feedback_orb_settings_panel
description: "the orb opens a settings/about/help panel; device management is a separate PAGE inside that panel — never the orb's direct action"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 0b164fd9-062c-407e-b4bb-8f6be8d1982d
---

The orb (chrome app icon) is the entry point to a settings/about/help panel.
Device management (add/remove/rename devices) lives on its own page WITHIN that panel — the orb must not jump straight into a device flow.

**Why:** the orb is general-purpose chrome; binding it directly to one feature (the interim orb→AddDevice / orb→JOIN-mode wiring from pairing v1, `photon_app.rs on_orb_click`) doesn't scale as more settings arrive.

**How to apply:** when building or touching orb behaviour, route thru the panel: orb → panel → Devices page → add/remove flows (the flows themselves are charted in docs/device-lifecycle.md). Treat the current direct wiring as scaffolding to be dismantled, not a pattern to extend.

**Corollary (2026-07-04): "back is back, the orb is settings."** The orb NEVER carries a context action (cancel, toggle, back) — navigation is a dedicated back control. Shipped ed93ce2: the add-device screen got a real "‹ Contacts" back button (same idiom/hit-id as the Conversation back), and the orb's AddDevice-cancel + Launch-JOIN-toggle arms were removed. Only the INTERIM orb→AddDevice entry on Ready remains, and only until the settings panel + the attest probe-then-branch flow exist (probe-then-branch: after the ~1s proof, if the handle already has a fleet and this device isn't a member, route to "add this device" instead of the permanence warning — so typing your handle on a new device self-discovers the add-device path, retiring the orb entry entirely).

Related: [[project_keyring_design]], [[project_fleet_routing_scale]].
