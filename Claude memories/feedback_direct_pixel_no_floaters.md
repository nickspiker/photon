---
name: feedback_direct_pixel_no_floaters
description: "Rendering doctrine — DIRECT PIXEL ACCESS ONLY, no GPU shaders / vertex triangles / float pipeline (\"no floaters\")"
metadata:
  type: feedback
---

Photon's rendering is DIRECT PIXEL ACCESS ONLY — write RGB(A) into a CPU buffer (softbuffer / fluor's direct-pixel model), never a GPU shader/vertex/triangle pipeline.

**Why:** Nick 2026-07-09, on finding a full-screen-triangle shader blit on Linux: "THERE WILL BE NO FLOATERS IN MY PIPELINE! DIRECT PIXEL ACCESS ONLY!" A "floater" = floating-point GPU pipeline. Same integer/direct ethos as Eagle time, VSF and fluor's compositor: deterministic integer pixels, not float rasterization.

**How to apply:** use the softbuffer renderers (src/ui/renderer_linux_softbuffer.rs etc.); any wgpu/`.wgsl`/RenderPipeline/vertex/fragment code is a floater to rip out, not extend (the old wgpu renderer is gone). A platform present path uploads the finished CPU pixel buffer, never rasterizes geometry.
