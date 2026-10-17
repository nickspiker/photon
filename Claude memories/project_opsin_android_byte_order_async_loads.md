---
name: project_opsin_android_byte_order_async_loads
description: "2026-10-05: opsin in photon on Android rendered red-for-blue — every opsin pixel packer now routes thru fluor::theme::fmt; opsin loads/convert off the UI thread; photon viewer draws fetch/decode bars"
metadata:
  type: project
---

**Swap:** opsin packed pixels in desktop byte order (`view::argb`, `encode_pixels`, render.rs, panel.rs); fluor's Android surface is RGBA_8888, and `fluor::theme::fmt` (R↔B on Android, identity elsewhere, its own inverse) is the one adapter. Every opsin packer now wraps it; `unpack` applies it first.
**Hangs:** `OpsinApp` loads on a thread (`start_load` / `finish_load`, token-guarded); `V` convert is threaded (`convert_to_vsf`); `load_image_folded_with(..., &progress)` reports coarse stages; the view's `busy` bar draws with or without an image.
**photon viewer:** `Viewer.decode_frac` (Q16 atomic) repaints on the drain edge, never a timer; a fetch bar shows the chunk count before decode. The attachment seal (`blob_store_any`) moved into the prepare job.
Not field-verified on Android yet.

**Why:** Nick 2026-10-05: DNG previews looked "super bad, red blue swap", and loads "just looks like a big UI hang".
**How to apply:** any NEW pixel packer (opsin or elsewhere) goes thru `fluor::theme::fmt`; image work stays off the UI thread with edge-driven progress. Related: [[project_viewer_is_opsin]], [[project_android_color_pipeline_floor]].
