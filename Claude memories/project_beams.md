---
name: project_beams
description: BEAMS (video) DECIDED + BUILD STARTED 2026-10-10 — H.264 on the wire (hardware on phones, OpenH264 on desktops), no keyframes on the phone (intra refresh), the Android ISP stood aside with reported gains as pre-emphasis, sensor RGB under γ2 with the VSF characterization entry (creative/absolute/relative) in-band, integer receive chain + blue-noise truncation; Linux desktop slice built, phone capture next
metadata:
  type: project
---

**The design is docs/beams.md (canonical). The evening's reasoning, for the record:**
- Nick: "no AV1 unless hardware supported it… and even then h264 battery" — H.264 is the wire codec; AV1 costs several times the CPU in software for a third less bandwidth a beam doesn't need. "C is fine. I'd stick to H.264."
- Nick: "this whole one chunk of high data every once in a while I'm not keen on" — no keyframes after the first on the phone; intra refresh (rolling stripe); OpenH264 lacks refresh so the DESKTOP keyframes every 600 frames and on request, allowed there only.
- Nick: "Absolute, scene was under tungsten, image should look yellow… lens shading off too" — the ISP stands aside (AWB off, identity matrix, our γ2 tonemap curve, shading/NR/edge/aberration/distortion/stabilization OFF; hot-pixel + AE stay ON). The gains are applied by the ISP as PRE-EMPHASIS and reported per frame, divided back out on receipt (the free bit against 8-bit quantization of sensor space); the MATRIX stays off the phone (negative lobes would clip).
- Nick: labels = VSF's imaging taxonomy: `creative` for mac/unknown, `absolute` for Android with the maker's matrix (SENSOR_COLOR_TRANSFORM, what lumis reads), `relative` for a chameleon-scanned profile. Never an ad-hoc flag.
- Nick: "multiplying in isize and a bitshift" (i32, in fact) — no float in the pixel path; "I wouldn't round and I'd use a stochastic" — blue-noise tile (void-and-cluster, 64×64, origin hashed per frame), truncate; error diffusion is for audio, it crawls on video.
- Dither hides OUR steps only, never codec artifacts (blocking, ringing, smearing, chroma bleed).

**Built 2026-10-10 (commits 5ce6b384 design, 69d6b390 stages 1+2+receive chain, then the Linux desktop slice):** wave/beam.rs (transport: BEAM_MAGIC 0xCA, own StepChains, one frame = one fountain object, self-describing symbols, in-band Info), wave/h264.rs (OpenH264 on x86_64 Linux + macOS, AVAILABLE false elsewhere), wave/beam_colour.rs (integer chain), wave/beam_session.rs (Receiver with the engine, Sender from a camera or the phone's encoded feed, Picture mailbox), platform/camera_v4l2.rs (Linux YUYV→I420), the Beam button live on the desktop wave screen, the picture painted under the avatars in the wave square.

**Toolchain facts (the codec scout):** openh264-sys2 builds C++ via cc and needs a C++ stdlib at link; Redox has no build arm; x86_64 Windows lacks mingw64-gcc-c++ on the builder; gnullvm needs CXXSTDLIB=c++; aarch64 Linux sysroot lacks libstdc++; the arch gate passes untouched. Opus is C (fine everywhere); OpenH264 is C++ (not yet everywhere).

**Field verify pending:** desktop↔laptop over the LAN with a V4L2 camera. Then the phone (TICKETS "BEAMS — the rest").
Related: [[project_waves]], [[project_wave_beam_transition]], [[project_wave_canonical_container]].
