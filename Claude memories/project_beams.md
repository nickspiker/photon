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

G2T BEAM STREAM (planned 2026-10-20, after the Android HDR merge; Nick: "tag the stream as g2t if we have the luxury"): today a beam's colour entry carries the camera matrix + a transfer (vsf spectral_image::Transfer: linear/srgb/gamma2/gamma22; photon beam_colour::Transfer uses Gamma2/Srgb) — no g2t. VSF names g2t in the frame-encoding vocab (vsf::visual::Encoding::Gamma2Tanh "g2t"), which argues the curve rides the STREAM's encoding, the matrix the camera (the vsf session's open entry-vs-frames question). Receive first (g2t codes → G2T light → matrix → g2t buffer; the peak drives the HDR request), then send: Camera2 10-bit HLG where offered → light → g2t codes in the same 8-bit OpenH264 stream (headroom to 9.7×). Tag g2t only when light actually passes white — g2t spends 194 codes below white where γ2 spends 255.

**2026-10-21 audit (Nick asked "we keep both ends of beams, audio and video, and reconstruct missing?"):** AUDIO yes — every wave auto-keeps PHWAVE9 (ch0 own mic Opus 160k, ch1 far wire + fills; drain ≤2.5 s at hangup; fleet-replicated under `waves.hold`, unmetered only) ≈ 2 MB/min. VIDEO NO — beams record nothing (docs/beams.md stage 6 "Later"); no fill channel for video (FEC 1/8 + intra refresh only). Taps would be BeamTx::send_frame (beam.rs) + take_frames. NO expiry exists for anything (docs/retention.md designed, unbuilt; TICKETS.md:49 slider names beams as a category). Beam size if kept: 720p30 ≈ 21 MB/min one way (beam_ladder::bitrate_for). Hook for expiry = blob store (storage/mod.rs blob_store_any / blob_delete) + replication gate (replicate_sweep) so evicted blobs aren't re-fetched.
