---
name: project_button_one_renderer
description: ONE pill/button renderer lives in fluor; photon never hand-rolls squircle buttons
metadata: 
  node_type: memory
  type: project
  originSessionId: d83fbeaf-685c-4da4-8647-7b49de82fd2c
---

Button migration DONE 2026-08-19 (fluor bb3d099 + photon c2b2079, call overlay 519f9e2).

THE RULE: every button/pill draws through fluor::widgets::Button — never a hand-rolled squircle in photon.
User mandate, verbatim: "Anything that draws a button should use the button, no hand rolled shit... we need to use fluor's GUI" (the pills had "spread like a virus").

Two fluor paths, both the SAME look (squircle + two-tone raised edge + no-floor 1/32·font_size stroke + disabled-label dim):
- **Retained** `Button` (click-counter, in visit_app_widgets walk, hover via overlay-delta pass): attest/+/send, and the CROSS-SCREEN call overlay (call_action/decline/start + status chip) — registered before the per-state matches in visit_app_widgets, painted front-first, hit re-stamped last via stamp_hit_into.
- **Immediate** `Button::draw_pill_immediate` (stateless, fit-to-slot font shrink/grow, hover from a per-frame set_stub_hover publish): photon's ~28 settings/launch/contact-panel action pills. draw_stub_pill* are now THIN adapters over it (read stub_hover, compute pressed, gate hit_map on enabled) — kept, not deleted; the 48-slot immediate dispatch is untouched.

Why it bit before: hand-rolled pills never read hover_hit (the overlay-delta pass only tints retained widgets), so every settings/launch/call pill was hover-dead — same class as the call-button hover report. draw_pill_immediate's state fill mirrors effective_fill (pressed > hover > idle, same brightness factors).

If you add a new button/pill: retained Button if it's few + cross-screen/focusable; else draw_stub_pill* (→ draw_pill_immediate). NEVER call draw_squircle_pill_f directly for a button (the settings_widgets Checkbox squircle is a distinct control, not a pill). See [[feedback_direct_pixel_no_floaters]].
