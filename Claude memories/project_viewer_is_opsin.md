---
name: project_viewer_is_opsin
description: "2026-09-17 photon's image viewer IS opsin's embeddable opsin::view::View (panel, HUD, keys) hosted in the conversation screen; wiring + what to keep in sync"
metadata:
  type: project
---

Nick 2026-09-17: replace the "weird tiered image viewer" with opsin's own interface (DNG, exposure slider, etc.), plus Back/Escape and a top-left back arrow. The old micro→preview→Original tiers, ±½-stop pills and external opsin spawn are gone.

**opsin side:** `app.rs` split into `view.rs` (`View` = the whole viewer: pan/zoom/crop/rotate, navigator panel, histogram, exposure −4..+12 EV, HUD, keys) and a thin `OpsinApp` shell. `View` is area-relative (`set_area`), takes host-allocated hit ids (`View::new(loaded, &mut hit_counter)`), renders into the host's hit map, answers `on_event` with `Handled | Close | Pass`. Feature `view` = no winit (Android); `viewer` = view + winit. Wheel pixels = pan, lines = zoom.

**photon side (`viewer.rs`):** `request_view_decode` → worker decodes (opsin, or the image-crate legacy path for PNG/GIF/BMP) → `drain_img_view` builds the View on ids `viewer_view_base`. driver.rs forwards input to the view FIRST (except photon's Back/Save pills); `visit_app_widgets` yields the view's widgets. Export pill relabelled "Save" → `viewer_save`.

**Keep in sync:** a new View widget → bump `VIEW_HIT_IDS` (16) if the count passes it. View labels stay opsin's English (instrument labels) except Save; readouts are arabic decimals.
Related: [[project_attachments]], [[project_button_one_renderer]].
