---
name: project_party_colour_perceptual
description: Conversation party colours are a placeholder; swap to perceptual L≈50% via vsf spectral/LMS
metadata: 
  node_type: memory
  type: project
  originSessionId: 3ca9e756-6713-4b94-b9d2-5e57659c52a8
---

Per-party message text colour in the conversation view is deterministic from the handle hash, but the current implementation is a deliberate PLACEHOLDER: `party_colour(handle_hash)` in src/ui/photon_app.rs derives hue from the hash with fixed saturation + sRGB lightness via plain HSL→RGB.

**The intended design:** hue + saturation vary by handle, but perceived lightness is pinned to ~50% so every party's colour reads at equal brightness (nobody gets an unreadably dark or blinding name). That requires generating/converting through a perceptually-uniform space, not sRGB — the work routes through the vsf spectral/LMS pipeline at ../vsf/src/colour/spectral/ (e.g. LMS2PHOTOPIC in constants.rs, which was a placeholder of [1,1,0] needing real photopic weights).

**How to apply:** when doing the perceptual colour pass, replace only the body of `party_colour` (signature kept stable as a drop-in) — hue+sat from the handle, perceived L≈50% via spectral/LMS. The conversation render already calls it for both our colour (our identity_seed) and theirs (contact.handle_hash). Related: [[project_android_color_pipeline_floor]] (the colour pipeline work), messaging landed in commit b6974c9.

**CLOSED 2026-10-06 (Nick's ruling):** 50% means PHYSICAL — half the illuminant-E power reflected — so the Y = 0.5 LINEAR plane stays and L* is not the frame. What changed: the draw is now UNIFORM BY AREA over the Y = 0.5 slice of (VSF cube ∩ Rec.2020 cube), digest-deterministic rejection sampling, replacing the hue-ray + √-chroma scheme that pushed every colour to the gamut wall ("not sure why we are clipping some to the edge and skewing away from grey"). The settings label dropped its placeholder wording. Shipped in `party_colour` (src/ui/photon_app.rs).
