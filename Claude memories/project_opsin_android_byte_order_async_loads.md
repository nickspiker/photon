---
name: project_opsin_android_byte_order_async_loads
description: "2026-10-05 opsin inside photon on Android rendered red-for-blue (opsin packed pixels desktop-order; fluor's Android surface is RGBA) — fixed by routing every opsin-packed pixel thru fluor::theme::fmt; same day opsin's loads/convert went off the UI thread behind its busy bar, and photon's viewer draws a fetch/decode bar"
metadata:
  type: project
---

**Nick 2026-10-05:** "the colour for the image preview for DNG ... in Opsin on Android it looks super bad, red blue swap or something" + "show a progress bar for fetches and transfers ... right now it just looks like a big UI hang, and can we make it not hang?"

**Swap, convicted:** opsin packed α+darkness pixels in desktop byte order everywhere it packs its own (`view::argb`, `encode_pixels`, `render.rs`, `panel.rs` pack/unpack/histogram). fluor's Android surface is RGBA_8888, so photon routes its own previews thru `fluor::theme::fmt` (R↔B on Android, identity elsewhere, const fn, its own inverse) — opsin never did. The display matrix WAS applied (`to_linear_in` → `display_matrix`); only the byte order was wrong. Fix: every opsin packer wraps `fluor::theme::fmt(...)`; `unpack` applies it first. Any NEW pixel packer in opsin must do the same.

**Hangs, fixed in opsin:** `OpsinApp` loads on a thread (`start_load` / `finish_load`, token-guarded, arrow walk skips undecodable files by stepping on), the launch path waits in `pending_open` until the wake sender exists so the window shows first; `V` convert runs on a thread (`convert_to_vsf` free fn); the calibrate reload goes thru `Msg::Open`. `load_image_folded_with(path, edge, keep, &progress)` reports coarse stages (0.05 / 0.6 / 1.0). Messages: `Msg::Loaded(token, path, result)`, `LoadProgress(token, f)`, `ConvertDone`. The view's `busy` bar draws with or without an image.

**photon viewer:** `Viewer.decode_frac` (Arc<AtomicU32>, Q16) written by the decode job, repainted on the edge in the drain (never a timer); the pre-decode arm draws a bar = chunk count while fetching (`attach_chunk_progress`), the decode stage once held. The attachment seal (`blob_store_any`) moved into the prepare job; `AttachPrepared` carries `hash` + `manifest`.

**Open:** the other opsin session's in-progress export/RCD work was committed for the v112 deploy (a45e4e6) and built on here. Not field-verified on Android yet.

Related: [[project_viewer_is_opsin]], [[project_android_color_pipeline_floor]], [[feedback_no_time_based_ui]], [[project_attachments]].
