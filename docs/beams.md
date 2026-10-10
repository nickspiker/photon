# Beams — video, the beam of light

A **beam** is video: the beam of light, aimed at one person (docs/lexicon.md). A **wave** is voice. A beam is a wave with a second track; everything a wave already does — the ceremony, the ring, the key ratchet, the sealed datagrams, the fountain-code repair, the loss loop, the fleet lifecycle (docs/waves.md) — carries the picture unchanged. This document pins what is specific to the picture.

**Status:** DECIDED 2026-10-10 (Nick + Claude, the evening's design pass). BUILT the same night: stage 1 (OpenH264 on x86_64 Linux + macOS, `wave/h264.rs`), stage 2 (the transport, `wave/beam.rs`), the receive chain of §4 (`wave/beam_colour.rs`, integer, blue-noise truncation), and the LINUX DESKTOP SLICE: `wave/beam_session.rs` (receiver up with every wave engine; sender from a camera), `platform/camera_v4l2.rs`, the Beam button live on the desktop wave screen, the peer's picture painted in the wave square. Stage list at the end.

## 1. The decisions, in one place

| Question | Decision | Why |
|---|---|---|
| Codec on the wire | **H.264**, constrained baseline / main, 8-bit 4:2:0 | Every phone we support encodes and decodes it in hardware; battery and heat are a non-issue; AV1 in software costs several times the CPU for a third less bandwidth a beam doesn't need. AV1 is revisited only when hardware AV1 encode is common. |
| Phones | **Hardware both ways**, MediaCodec | The vendor's block, tuned for the device — the same argument that took Android's own echo canceller for waves. |
| Desktops | **Software both ways**, Cisco OpenH264 (C, BSD) | A desktop has the CPU; x264 is GPL and the project is MIT/Apache; there is no mature pure-Rust H.264. Hardware desktop paths (VideoToolbox, Media Foundation, VA-API) can come later or never. |
| Keyframes | **Never, after the first.** Intra refresh (gradual decoder refresh): a rolling stripe of macroblocks is intra-coded every frame, so the whole picture is refreshed every N frames and the bitrate stays level. | Nick: "this whole one chunk of high data every once in a while I'm not keen on." A corruption heals by wipe within N frames instead of waiting for a keyframe. Long-term reference frames are the complement where the encoder exposes them. |
| Rate control | Constant bitrate, low-latency mode, a per-frame size cap | The wave's loss loop picks the rate; the encoder must honour it frame by frame, not on average. |
| Bitrate | ~300–800 kbit/s between 360p and 480p at 30 fps; the loss loop's ladder | A smaller picture at a healthy quantizer beats a larger one starved. |
| Colour on the phone | **The ISP stands aside.** AWB off, matrix identity, our own γ2 tonemap curve, lens shading / noise reduction / edge / aberration / distortion / stabilization / effects / scene modes all OFF. Hot-pixel correction and auto exposure stay ON. | Absolute: a scene under tungsten looks yellow. Level is not colour; a dead pixel in a beam is only ugly. |
| White balance gains | **Applied by the ISP as pre-emphasis, reported per frame, divided back out on receipt.** | Fills all three 8-bit channels before the codec (the one free bit); still absolute because the gains are data on the wire, not an opinion in the pixels. The matrix stays OFF the phone because its negative lobes would clip real colours; gains cannot clip except highlights any camera clips. |
| Transfer on the wire | **γ2** (VSF RGB's own curve): the ISP's tonemap curve is set to `out = sqrt(in)`; linear is the square. | Linear 8-bit crushes shadows; sRGB's curve costs a table; γ2 costs one multiply. |
| Colour space on the wire | **Demosaiced sensor RGB under γ2**, YCbCr 601 full range as the ISP emits it; the camera's **VSF characterization entry** (`vsf::spectral_image`: the camera→VSF-RGB matrix, `IdtClass`, `ProfileTier`, `Transfer`, provenance — serialized by `vsf::visual::characterization_fields`) rides the beam's signalling once; the gains ride every frame. | VSF RGB on the wire in everything but the matrix multiply, and the multiply is free on the receiver in integer. The label is VSF's own imaging taxonomy, never an ad-hoc flag (Nick 2026-10-10). |
| The colour label (VSF imaging taxonomy) | **mac / any camera we know nothing about:** class `creative`, tier `assumed`, transfer `srgb`. **Android with the maker's matrix** (SENSOR_COLOR_TRANSFORM, what lumis reads): class `absolute`, tier `model`, transfer `gamma2`. **Android (or any camera) with a chameleon target scan:** class `relative`, tier `unit`, transfer `gamma2` (`srgb` for a UVC camera scanned thru its own processing). | Nick: "creative for mac since we have zero clue what it really is, absolute for android with a known transform, relative for a scene-relative calibrated chameleon profile." `Transfer::Gamma2` already exists in vsf. |
| Desktops' cameras | **`creative` / `assumed` / `srgb`** unless scanned: AVFoundation (mac) exposes nothing; V4L2 / UVC (Linux, Windows) expose only auto-WB-off, a temperature, gamma, sharpness — pinned where they exist; a chameleon scan supplies the matrix when the user has scanned that camera. | There is no ISP to address outside Android. The phone is the end that can be made honest and the end that matters. |
| Receiver arithmetic | **Integer, no float in the pixel path.** YCbCr→RGB in Q12/i32 (8-bit in, 22 bits needed); γ2 decode = square (16-bit linear); sensor→VSF RGB in Q10–Q12/i32 with the reciprocal gains folded in, clamp after; then the existing VSF RGB → BT.2020 theme formatter. i32 with explicit headroom, never isize; i16 lanes with widening multiplies for SIMD. | Direct pixel, like the rest of fluor; the floats belong to chameleon's one-time solve and the once-per-beam constant conversion. |
| Final quantization | **No rounding. Blue-noise dither, truncate.** A precomputed 64×64 void-and-cluster tile, indexed by x,y modulo 64, its origin rotated per frame by a hash; added before the truncating shift at the ONE place it belongs — the final 8/10-bit display output. The 16-bit intermediate needs none. | Unbiased (the mean of floor(x+u) over uniform u is x); all-high-frequency noise, least visible; vectorizes trivially; no GPU. Error diffusion is right for audio (the wave gain's carried remainder) and wrong for a moving picture (serial, and its pattern crawls between frames). |
| What dither does NOT do | Hide codec artifacts. | Blocking, ringing, smearing, chroma bleed and keyframe pumping sit many code values high; a one-code noise floor covers only our own steps. What tames the codec is bitrate, H.264's in-loop deblocker, and a resolution the bitrate can afford. |
| Recording | **Deferred.** A beam mints `beam.video` beside `wave.audio` in the locked wave container (another track, rows are spans) when it lands; fleet replication and retention apply as to any blob. | docs/waves.md "Canonical container"; TICKETS retention horizons. |
| Relay media | **Unchanged gap.** Two carrier-NAT phones cannot beam, exactly as they cannot wave. | TICKETS "NAT relay tier". |

## 2. Why H.264 and not AV1 (the numbers that decided it)

- Compression: AV1 needs ~30–50 % fewer bits than H.264 at equal quality; at real-time low-latency settings, where most of AV1's tools are off, ~25–35 %.
- Software encode cost: AV1's real-time encoders run 3–10× the CPU of x264 at comparable presets; rav1e (pure Rust, already in the tree for stills) is the slowest of them, built for quality per bit. On a Snapdragon 855, x264 holds 640×480 at 30 fps on one core; rav1e fights for 320×240 at half that.
- Decode: nearly a wash — AV1 ~1.5–2× H.264 in software, both a few percent of a core at beam sizes.
- Hardware: both free when the silicon has them; H.264 encode is on every phone, AV1 encode only on the newest, and first-generation AV1 blocks draw comparable power at best.
- The trade is bits against watts, and a beam's few hundred kilobits are not worth several times the CPU. Every shipping messenger made the same call: AV1 on strong phones, hardware H.264 everywhere else.

## 3. The Android capture request (straight thru)

On a camera whose characteristics list `MANUAL_POST_PROCESSING` (FULL / LEVEL_3; each key is additionally gated by its own availability list):

```kotlin
set(CONTROL_AWB_MODE, CONTROL_AWB_MODE_OFF)
set(COLOR_CORRECTION_MODE, COLOR_CORRECTION_MODE_TRANSFORM_MATRIX)
set(COLOR_CORRECTION_GAINS, RggbChannelVector(rGain, gEven, gOdd, bGain))   // our measured white point, pre-emphasis, reported per frame
set(COLOR_CORRECTION_TRANSFORM, identity 3×3 of rationals)
set(TONEMAP_MODE, TONEMAP_MODE_CONTRAST_CURVE)
set(TONEMAP_CURVE, TonemapCurve(gamma2, gamma2, gamma2))   // (in,out) pairs, out = sqrt(in), up to TONEMAP_MAX_CURVE_POINTS
set(SHADING_MODE, SHADING_MODE_OFF)
set(NOISE_REDUCTION_MODE, NOISE_REDUCTION_MODE_OFF)
set(EDGE_MODE, EDGE_MODE_OFF)
set(COLOR_CORRECTION_ABERRATION_MODE, COLOR_CORRECTION_ABERRATION_MODE_OFF)
set(DISTORTION_CORRECTION_MODE, DISTORTION_CORRECTION_MODE_OFF)
set(CONTROL_VIDEO_STABILIZATION_MODE, CONTROL_VIDEO_STABILIZATION_MODE_OFF)
set(CONTROL_EFFECT_MODE, CONTROL_EFFECT_MODE_OFF)
set(CONTROL_SCENE_MODE, CONTROL_SCENE_MODE_DISABLED)
// CONTROL_AE_MODE stays ON; HOT_PIXEL_MODE stays ON; AF auto.
```

The pipeline is then black level → demosaic → gains → our curve → YCbCr. The `CaptureResult` echoes the mode, gains, transform and curve the HAL actually applied, per frame: the readback is the truth, and a chameleon target scan through the whole beam pipeline is the one-time proof per phone that the chain is straight. The gains come from a few seconds of AWB ON at beam start (read back and frozen) or our own grey-world on a downscaled frame.

Where the phone lacks the capability: AWB ON, read the applied gains and transform back per frame and invert them on receipt; a fully reported pipeline, not a passthru, which is still better than a passthru nobody can verify.

RAW is lumis's road, not the beam's: RAW_SENSOR at 30 fps means our own demosaic in real time, and the ISP's demosaic is the one thing worth taking from it for a live picture.

## 4. The receive chain

```
H.264 NALs ──decode──▶ Y Cb Cr (8-bit, 4:2:0)
   │ 3×3 Q12 + offsets (601 full range), add half? NO — see dither; shift 12 → R'G'B' γ2, 8-bit → widen
   │ square → linear R G B, 16-bit
   │ 3×3 Q10–Q12 (sensor → VSF RGB) with 1/gains folded in, signed, clamp after → VSF RGB linear 16-bit
   │ theme formatter: VSF RGB → BT.2020 display encode (existing, integer)
   │ + blue-noise tile (x,y mod 64, origin hashed per frame), truncate → 8/10-bit surface
   ▼
fluor direct blit into the wave screen's square
```

The receiver branches on the characterization's `Transfer` and class: `gamma2` → the square; `srgb` → a 256-entry inverse table; a `creative`/`assumed` entry carries the assumed sRGB→VSF-RGB matrix and gains of one; a `relative`/`unit` entry carries the scanned matrix; an `absolute`/`model` entry carries the maker's. The pixel path never asks which camera it was — only what the entry says.

## 5. How H.264 blocks, so the artifacts are expected

16×16 macroblocks, predicted intra (from neighbours) or inter (a motion-shifted patch of a reference), the residual transformed in 4×4 (and 8×8 in High) integer blocks and quantized; the step doubles every 6 QP. At beam rates: **blocking** at block boundaries (the normative in-loop deblocker smooths it, keyed to QP), **ringing / mosquitoes** beside sharp edges, **smearing** of fine texture (high coefficients go first), **chroma bleed** (half-resolution, coarser-quantized colour — which the receiver's matrix then amplifies), and **keyframe pumping**, which intra refresh removes.

## 6. Stages (build order)

1. **Codec substrate (desktop).** OpenH264 as a desktop-only dependency through the arch gate; encode + decode of I420 in software; intra refresh if the library has it, else periodic keyframes on the desktop side only. A round-trip test.
2. **The video track.** A second media kind beside the wave's audio and its recording-fill plane, built the way the fill plane is: its own packet magic, its own sequence space, its own StepChains from a domain-separated secret (`"PHOTON_WAVE_v1 beam video"`), demuxed by magic at the media sink before the audio engine sees it. One encoded frame = one fountain-code window, fragmented to MTU-sized symbols with repair symbols in proportion; its own rate rung under the loss loop. **Nothing is added to the offer/answer**: the beam describes itself in-band — a `beam info` packet (geometry, fps, codec, the VSF characterization entry) at start and once a second, like intra refresh for metadata, so a beam can start mid-wave and a late joiner needs nothing from signalling; the per-frame gains ride each frame's header.
3. **Phone capture + encode.** Camera2 with the request above → YUV_420_888 → MediaCodec H.264 (low latency, CBR, intra refresh, keyframe interval never) → NALs to Rust over JNI, the same registration shape as the AAudio bridge. CAMERA permission with the POST_NOTIFICATIONS request pattern.
4. **Receive + render.** Decode (MediaCodec on the phone, OpenH264 on the desktop) → the integer chain of §4 → blue-noise dither → blit into the wave square. Beam buttons go live.
5. **Desktop capture.** V4L2 on Linux (opsin's live path knows the way), AVFoundation on mac and Media Foundation on Windows as "assumed sRGB" sources.
6. **Later:** `beam.video` recording track; long-term reference frames; 10-bit HEVC only if a target scan proves 8-bit sensor-space colour loses something a beam can see; hardware desktop codecs.

Related: docs/waves.md (the ceremony, container, lifecycle), docs/lexicon.md (beam), docs/PT.md "The bitmapless transfer" (what a kept beam will ride), docs/retention.md.
