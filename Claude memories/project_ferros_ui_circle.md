---
name: project_ferros_ui_circle
description: "ferros Photon UI laid round the safe circle (2026-11-18): orb under the camera = the face, beads, title arc pages; BUILT, glass look pending"
metadata:
  type: project
---

Built 2026-11-18 (photon f0addea6, 2214e36b, 9825d5fb, c616ef3c; fluor ec68b08 `set_orb_place`), checked only in a desktop render of the Pro's panel; the Pro was paused for the camera agent, so nothing was looked at on the glass.
- `ui::safe_circle()` is set only on ferros (EV_SAFE thru `on_safe_area`, logged "SAFE:"; first frame guessed by `SafeCircle::of_panel`); every ferros-only placement reads it, so desktop and Android are untouched.
- `ui::safe_layout::OrbLayout`: orb at the circle's top under the camera, Wave/Beam beads on the arc thru its centre; `soft_cap` keeps 25%..400% on the glass; `bar_floor` + `soft_max` keep the compose bar inside the circle, clear of the helm.
- Ready: the orb is our own face (brand until signed in); no big avatar. Conversation: orb = their face, name under it, no back text, no blind. Unread = dot, not magenta ring.
- Settings and conversation panel: no rail; title arc (cos² α, spring ω = 1/4 a frame), tap title = picker, tap orb or helm Back = close. Updates dropped from ferros's pages.
- Open: 400% + keyboard crowds the compose bar into the name; page bodies can draw below the content floor at 400%; the orb's dot means "another conversation waits"; orb-closes-page contradicts [[feedback_orb_settings_panel]] (owner's call).
**How to apply:** look at it on the glass first (03-before-now.vsf, 04-before-ready.vsf are the before captures); the scratch desktop harness lives only in the session scratchpad.
