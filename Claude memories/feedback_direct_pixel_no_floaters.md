---
name: feedback_direct_pixel_no_floaters
description: "Rendering doctrine — DIRECT PIXEL ACCESS ONLY, no GPU shaders / vertex triangles / float pipeline (\"no floaters\")"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: cf9fbed2-aa87-4ebe-b7cf-fca16d308f75
---

Photon's rendering is DIRECT PIXEL ACCESS ONLY — write RGB(A) into a CPU buffer (softbuffer / fluor's direct-pixel model), never a GPU shader/vertex/triangle pipeline.

**Why:** the user's doctrine, stated emphatically 2026-07-09 on finding "a sneaky fucking triangle in the shader for linux": "THERE WILL BE NO FLOATERS IN MY PIPELINE! DIRECT PIXEL ACCESS ONLY!" A "floater" = floating-point GPU pipeline (shaders, vertices, the classic full-screen triangle blit). It's the same integer/direct ethos behind Eagle-time oscillations, VSF, and the fluor under-blend compositor — deterministic integer pixels, not float rasterization.

**How to apply:** prefer the softbuffer renderers (renderer_linux_softbuffer.rs etc.); treat renderer_linux_wgpu.rs and any wgpu/`.wgsl`/RenderPipeline/vertex/fragment code as suspect — a GPU-shader blit path is a floater to rip out, not extend. If a platform needs a present path, it should upload the finished CPU pixel buffer, not rasterize geometry. See [[project_android_color_pipeline_floor]] (the panel-accuracy work is also about exact pixels, not GPU colour math).
