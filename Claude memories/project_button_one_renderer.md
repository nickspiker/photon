---
name: project_button_one_renderer
description: ONE pill/button renderer lives in fluor; photon never hand-rolls squircle buttons; every reusable control is a fluor::widgets primitive
metadata:
  type: project
---

Button migration DONE 2026-08-19 (fluor bb3d099 + photon c2b2079).
THE RULE: every button/pill draws through fluor::widgets::Button — never a hand-rolled squircle in photon. Nick, verbatim: "Anything that draws a button should use the button, no hand rolled shit... we need to use fluor's GUI."

Two fluor paths, same look:
- **Retained** `Button` (in the visit_app_widgets walk, hover via the overlay-delta pass): few + cross-screen/focusable buttons (attest/+/send, the wave overlay).
- **Immediate** `Button::draw_pill_immediate` (stateless, fit-to-slot font): settings/launch/contact-panel pills, reached via photon's thin `draw_stub_pill*` adapters.

**Why:** hand-rolled pills never read hover_hit, so every one was hover-dead.
**How to apply:** new button → retained Button if few + cross-screen, else `draw_stub_pill*`; NEVER call draw_squircle_pill_f directly for a button.
WIDER RULE (2026-08-19): every reusable control lives in fluor::widgets — Button, Textbox, MultiTextbox, Slider, Dropdown, Checkbox. Radio does not exist yet: build it in fluor when a radio group first needs it, never in the host. See [[feedback_direct_pixel_no_floaters]], [[project_textbox_one_registry]].
