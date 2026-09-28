---
name: project-audio-picker-levels
description: "2026-09-28 DESIGN (unbuilt): independent mic + speaker pickers (Bluetooth included, constraints labelled not hidden) and live TX/RX level bars; stop-scaled bars, linear colour (green < ½ FS, yellow at ½, red at clip); picker shows ONLY the selected mic live, others grey — no rotation"
metadata:
  node_type: memory
  type: project
  originSessionId: 87fd33b1-f610-46f9-b464-4fe0dd1e3280
  modified: 2026-09-28T17:57:20.908Z
---

Nick 2026-09-28 decisions:
- **Pickers:** mic and speaker chosen independently, Bluetooth included (his case: phone mic + BT speaker, which rides A2DP full quality, no echo). Two lists, each starting with Auto; a choice that forces the other side SAYS so (BT headset mic → output to that headset at call quality; earpiece → latency cost on fallback phones like Emma's). No pre-grouped pairs.
- **Level readout:** TX = what the far side gets (post level plan), RX = what we render before the OS volume. Numbers per numeral mode: Arabic = dBFS (his explicit ask, overrides stops-not-dB for this label only), dozenal = linear fraction of full scale in dozenal digits, hex = 0.xx linear.
- **Bars:** STOP-SCALED (one tick per halving). COLOUR IS LINEAR in amplitude: green below ½ full scale, yellow at exactly ½, red at 100% (clipping), yellow→red blend between.
- **Picker mic circles:** radius grows per stop, same colour rule. Every input the phone can capture AT ONCE shows live (e.g. built-in mics exposed as one multi-channel capture — probe for it); any input that can't be read concurrently is a grey circle unless it is the selected one. NEVER rotate/round-robin probe (Nick: "rotating would add confusion"). Desktop: all inputs can be live.
Android facts gathered: AAudio setDeviceId per stream allows independent in/out; usually one active physical input per app; BT mic forces HFP/SCO (call quality, battery); earpiece only via voice-usage/communication-device routing; A2DP adds ~100–250 ms that phones may misreport (named playout L needs a field check).
Related: [[project_media_fallback_rocker]], [[project_level_plan_reaim]], [[feedback_stops_not_db]], [[edges-not-timers]].
**BUILT 2026-09-28 — the wave FIELD instead of bars (Nick's design):** square at the top of the Active wave screen, their avatar ⅓ in from top-right, ours ⅓ in from bottom-left, each ¼ of the side across; audio age = (d² − r²)/k in doubled integer coords (NO sqrt, equal area per frame, ~5.1 s = 1024 frames), ages precomputed per square size (wave_field::FieldMap), colour tables per paint from agb_bytes (the card's function) × stop-scaled brightness (12 stops), sides ADDED in linear light, Q12 → γ2 LUT → fluor darkness, composited UNDER avatars/text via fluor::paint::flatten per row (rayon). Avatar rings = live level colour, fixed width; path-tier colour moved to a dot left of the status line. Data: wave::live rings fed by engine TX (post level plan) and RX (at queue_named), RX ages from audio::play_head(). 1080² paint ≈ 1.1 ms on the desktop, map build 6 ms. Mic/speaker pickers still unbuilt.
**REVISED 2026-09-28 (Nick):**
- The field is a fluor LAYER: full-brightness colour with darkness premultiplied by α, α = brightness γ-encoded. The speckle lands under it, and the panel paints no backdrop.
- Brightness = amplitude 1:1 in linear light (no stops), times a LINEAR fade to 0 at the reach.
- Reach = the other avatar's near edge = ONE SECOND (200 frames; live ring 256).
- The square is 24 ru.
- Ages are 8.8 fixed point, interpolated between frames; paint only per-row spans inside the reach.
- Avatar rings = the field's age-0 colour (NOT level colours).
- The path colour lives in the TL orb.
- Chrome layer flattened FIRST, speckle LAST (fluor flatten_chrome_into / flatten_bg_into).
- Android Save/Export → public Downloads/Photon via MediaStore.
