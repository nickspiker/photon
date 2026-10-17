---
name: project_beams
description: BEAMS (video) DECIDED 2026-10-10, desktop BUILT, phone capture + camera cycle BUILT 2026-10-14 — H.264, intra refresh on phones, ISP stood aside, VSF labels, integer receive chain; field verify pending
metadata:
  type: project
---

**docs/beams.md is canonical.** Nick's decisions (2026-10-10):
- H.264 on the wire ("C is fine. I'd stick to H.264"): hardware on phones, OpenH264 on desktops; no AV1.
- No keyframes after the first on phones (intra refresh); OpenH264 lacks refresh, so DESKTOPS keyframe every 600 frames and on request.
- The Android ISP stands aside (AWB off, identity matrix, our γ2 curve, shading/NR/edge/distortion/stabilization off; hot-pixel + AE on); gains applied as PRE-EMPHASIS, reported per frame, divided out on receipt; the matrix stays off the phone.
- Labels = VSF imaging taxonomy: `creative` (mac/unknown), `absolute` (Android with SENSOR_COLOR_TRANSFORM), `relative` (chameleon-scanned). Never an ad-hoc flag.
- No float in the pixel path (i32 multiply + shift); blue-noise truncation (64×64 void-and-cluster tile, origin hashed per frame), never error diffusion on video. Dither hides OUR steps only, never codec artifacts.

**Code:** wave/beam.rs (transport, BEAM_MAGIC 0xCA, own StepChains), wave/h264.rs (OpenH264 on x86_64 Linux + macOS), wave/beam_colour.rs, wave/beam_session.rs, wave/beam_ladder.rs, platform/camera_v4l2.rs, PhotonBeam (Android capture: fps ≤30, least crop, ≤5.1 MP; bands = capture + exact halves; self-view tap cycles lenses mid-beam).
**Toolchain:** OpenH264 is C++ (needs a C++ stdlib at link; missing on x86_64 Windows builder and aarch64 Linux sysroot); Opus is C.

**Convicted 2026-10-14 (keep):**
- vsf::colour matrix constants are COLUMN-major; ProfileEntry, lumis, opsin and the beam chain are row-major → `beam_colour::vsf_rows` at every entry (the "green beam").
- Samsung HAL drops AWB when handed a tonemap curve → the curve rides only the full-straight path.
- OpenH264 zeroed buffers paint green → heal from the last picture, else black.
- An answer sealed under only the NEW era can beat the sibling's chain-sync → non-offer signals also seal under the retired era; answerer re-sends until media flows.
- Beam off/on mid-wave restarted the step chain → `PARKED_TX` keeps one transport + frame counter per wave.
- Chameleon is a RUNTIME PLUGIN (dlopen, platform/chameleon_plugin.rs), never a Cargo dep: an optional path dep to a missing dir breaks every build.

Field verify pending: colour on Samsung/Mac/Pixel, Calibrate on a straight phone, LAN desktop↔laptop. Related: [[project_waves]], [[project_wave_beam_transition]], [[project_wave_canonical_container]].
