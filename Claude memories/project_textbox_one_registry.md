---
name: project_textbox_one_registry
description: adding a textbox = register in TWO walks only (visit_app_widgets + textboxes_mut); everything else inherits — never hand-list per concern
metadata:
  type: project
---

Since the 2026-07-15 rework (photon e760799 / fluor 1eb4749), register a new textbox in exactly two places in `src/ui/photon_app.rs`:
- `visit_app_widgets()` — the `&mut dyn Widget` registry. Hover, damage union (`damage_rect`), fluor click/key/focus/tab dispatch and the I-beam cursor all iterate it. Screen-gated: must mirror the render gate.
- `textboxes_mut()` — the `&mut Textbox` registry for tab cycle, IME, blinkey and pointer gestures (`textbox_by_hit_mut` → drag-select / double-click).

Everything else inherits.

**Why:** hover/damage/cursor each kept private widget lists, so every new box silently regressed ("new box misses hover/damage, blinkie stacks, full-screen redraws" — Nick: "designed wrong").
**How to apply:** never add a per-concern hand-list. Textbox pointer gestures are driven from photon's press/drag/release path; `on_activate` skips `dispatch_release` for textboxes so fluor's `on_click` can't clobber a drag selection.
