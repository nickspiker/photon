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
- Settings and conversation panel: no rail; title arc (cos² α, spring ω = 1/4 a frame, each title fitted to 3/4 width), tap title = picker, helm Back (Escape) alone closes: the orb never closes a page ([[feedback_orb_settings_panel]], f8849a2d). Updates dropped from ferros's pages. Pages draw on the frame's rows down to the floor only (cut there, hits too).
- Round 2 (2026-11-18): beads are round fluor Buttons (fluor 1721a6c `set_round`) with marks ∿ and 📹︎, dimmed when they cannot start; ComposeFit sizes the compose bar to the free space; the filter pill has its own band; selection pills are cut at the list edge (draw_stub_pill_banded); the pair's About page drops its picture; `FERROS_PAGES_IN_TEST` lets a desktop render show ferros's page list.
- Open: at 400% the Ready list runs past the floor, and unwrapped page lines (a Conversations heading, the panel's rows) run off the glass's sides; the compose bar is narrow at 400% (w − 2 units); the orb's dot means "another conversation waits" (owner's call).
**How to apply:** look at it on the glass first (03-before-now.vsf, 04-before-ready.vsf are the before captures); the scratch desktop harness lives only in the session scratchpad.
