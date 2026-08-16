---
name: project_textbox_one_registry
description: adding a textbox = register in TWO walks only (visit_app_widgets + textboxes_mut); everything else inherits — never hand-list per concern
metadata: 
  node_type: memory
  type: project
  originSessionId: 77046248-8b61-4358-9ed9-26d48a57df98
---

Textbox cross-cutting concerns route through ONE screen-gated walk after the 2026-07-15 rework (photon e760799 / fluor 1eb4749).

To add a textbox, register it in exactly TWO places in `src/ui/photon_app.rs`:
- `visit_app_widgets()` — the single `&mut dyn Widget` registry. Hover, the damage union (`damage_rect`), fluor click/key/focus/tab dispatch, and the I-beam (`cursor_for` via `hover_is_textbox`) ALL iterate this. Screen-gated (must mirror the render gate).
- `textboxes_mut()` — the `&mut Textbox` registry for tab-cycle, IME, blinkey, and pointer gestures (`textbox_by_hit_mut` → drag-select / double-click).

Everything else inherits. Do NOT add a per-concern hand-list (that was the recurring "new box misses hover/damage, blinkie stacks, []w full-screen redraws" bug the user called "designed wrong").

**Why:** hover/damage/cursor-icon each kept private widget lists; every new box silently regressed. Fix = route them through `visit_app_widgets`; `Widget` gained `damage_rect` + `is_text_input` (fluor) and `FluorApp::damage_rect` is now `&mut self` so the app folds widget damage through the same walk it dispatches on.

**How to apply:** register once in each walk; textbox pointer gestures (press→`pointer_press`, drag→`pointer_drag_to`, release→`pointer_release`) are driven from photon's press/drag/release event path — `on_activate` skips `dispatch_release` for textboxes so fluor's `on_click` can't clobber a drag selection.

Known-deferred (host-level, need on-device): resize-drag size "lag" is fluor's shift-wrap-during-drag optimisation (re-lays out on release); mac text "overscroll bounce" is trackpad momentum vs the scroll clamp. See [[project_party_colour_perceptual]] neighbours for other parked UI. Related: [[feedback_direct_pixel_no_floaters]].
