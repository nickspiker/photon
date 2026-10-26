# Beams — video, the beam of light

A **beam** is video: the beam of light, aimed at one person (docs/lexicon.md). A **wave** is voice. A beam is a wave with a second track; everything a wave already does — the ceremony, the ring, the key ratchet, the sealed datagrams, the fountain-code repair, the loss loop, the fleet lifecycle (docs/waves.md) — carries the picture unchanged. This document pins what is specific to the picture.

**Status:** DECIDED 2026-10-10 (Nick + Claude, the evening's design pass). BUILT the same night: stage 1 (OpenH264 on x86_64 Linux + macOS, `wave/h264.rs`), stage 2 (the transport, `wave/beam.rs`), the receive chain of §4 (`wave/beam_colour.rs`, integer, blue-noise truncation), and the LINUX DESKTOP SLICE: `wave/beam_session.rs` (receiver up with every wave engine; sender from a camera), `platform/camera_v4l2.rs`, the Beam button live on the desktop wave screen, the peer's picture painted in the wave square; and stage 3's SENDING half on the phone (`PhotonBeam.kt`: the straight-thru Camera2 request, MediaCodec with intra refresh and no periodic keyframes, access units into the track; the phone does not decode yet). Then the same night (Nick: "I'd build all the things"): the offer's BEAM FLAG (an optional `bm` field on the express offer — the ring says "incoming beam" and offers Beam back, which answers with the camera armed), the SELF-VIEW inset (the camera thru the receiver's own colour chain, so it shows what the far side gets), DECODE ON THE PHONE (OpenH264 statically linked into the Android build — one decoder everywhere; the phone still encodes in MediaCodec), and the MAC CAMERA (`platform/camera_avf.rs`: AVFoundation, NV12 → I420, `creative`/`assumed`/`srgb`, the camera grant on the microphone's pattern). Stage list at the end.

## 1. The decisions, in one place

| Question | Decision | Why |
|---|---|---|
| Codec on the wire | **H.264**, constrained baseline / main, 8-bit 4:2:0 | Every phone we support encodes and decodes it in hardware; battery and heat are a non-issue; AV1 in software costs several times the CPU for a third less bandwidth a beam doesn't need. AV1 is revisited only when hardware AV1 encode is common. |
| Phones | **Hardware both ways**, MediaCodec | The vendor's block, tuned for the device — the same argument that took Android's own echo canceller for waves. |
| Desktops | **Software both ways**, Cisco OpenH264 (C, BSD) | A desktop has the CPU; x264 is GPL and the project is MIT/Apache; there is no mature pure-Rust H.264. Hardware desktop paths (VideoToolbox, Media Foundation, VA-API) can come later or never. |
| Keyframes | **Never, after the first.** Intra refresh (gradual decoder refresh): a rolling stripe of macroblocks is intra-coded every frame, so the whole picture is refreshed every N frames and the bitrate stays level. | Nick: "this whole one chunk of high data every once in a while I'm not keen on." A corruption heals by wipe within N frames instead of waiting for a keyframe. Long-term reference frames are the complement where the encoder exposes them. |
| Rate control | Constant bitrate, low-latency mode, a per-frame size cap | The wave's loss loop picks the rate; the encoder must honour it frame by frame, not on average. |
| Bitrate | ~300–800 kbit/s between 360p and 480p at 30 fps; the loss loop's ladder | A smaller picture at a healthy quantizer beats a larger one starved. |
| Colour on the phone | **The ISP stands aside.** AWB off, matrix identity, our own γ2 tonemap curve, lens shading / noise reduction / edge / aberration / distortion / stabilization / effects / scene modes all OFF. Hot-pixel correction and auto exposure stay ON. **All or nothing:** only where the camera can do every colour key (manual post-processing, AWB off, a maker matrix, a contrast curve). Anywhere else the camera keeps its whole colour pipeline (AWB, matrix, tone curve, shading) and the frames are labelled creative/assumed sRGB. A half-taken request is worse than none: a Samsung handed our curve without manual post-processing switches its AWB off and sends green. | Absolute: a scene under tungsten looks yellow. Level is not colour; a dead pixel in a beam is only ugly. |
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
H.264 NALs ──decode──▶ Y Cb Cr (8-bit, 4:2:0, 601 full range)
   │ 3×3 Q12 + offsets → R'G'B' codes, 8-bit
   │ the entry's transfer, a 256-entry table → light, Q16 (white = 65536)
   │ 3×3 Q10 (camera → VSF RGB → Rec.2020) with 1/gains folded in, signed, i64, clamp after:
   │   at white for an SDR entry, at the buffer's 9.7× for a g2t entry (an HDR beam keeps its light past white)
   │ light → g2t code × 256 by table
   │ + blue-noise tile (x,y mod 64, origin hashed per frame), truncate → 8-bit g2t Rec.2020
   ▼
fluor direct blit into the wave screen's square — the buffer's own encoding, no conversion at the blit
```

The receiver branches on the characterization's `Transfer` and class: `gamma2` → the square; `srgb` → the sRGB curve; `g2t` → fluor's `G2T_LIGHT_Q16` (an HDR beam, §5f); a `creative`/`assumed` entry carries the assumed sRGB→VSF-RGB matrix and gains of one; a `relative`/`unit` entry carries the scanned matrix; an `absolute`/`model` entry carries the maker's. The pixel path never asks which camera it was — only what the entry says.

## 5. How H.264 blocks, so the artifacts are expected

16×16 macroblocks, predicted intra (from neighbours) or inter (a motion-shifted patch of a reference), the residual transformed in 4×4 (and 8×8 in High) integer blocks and quantized; the step doubles every 6 QP. At beam rates: **blocking** at block boundaries (the normative in-loop deblocker smooths it, keyed to QP), **ringing / mosquitoes** beside sharp edges, **smearing** of fine texture (high coefficients go first), **chroma bleed** (half-resolution, coarser-quantized colour — which the receiver's matrix then amplifies), and **keyframe pumping**, which intra refresh removes.

## 5b. Orientation and layout (decided + built 2026-10-13)

Nick: "We capture and send at whatever orientation the sensor is, we don't flip anything. We do however record the orientation from our screen orientation code (don't use Android's)… flip it on demand based on the rotation set by the frame that we send." The wire stays sensor-native. A `Kind::Orient` packet `[2, quarter turns clockwise]` names how far the receiver turns the picture so the world's up is up: the camera's mounting angle (`SENSOR_ORIENTATION`) against the device's rotation from photon's OWN gravity snap (`PhotonActivity.deviceRotationCw`, the same 7 m/s² deadband that drives the UI, kept current even when rotation is locked), the front camera's sense reversed (Camera2's JPEG-orientation rule). Sent at start, on every change, and with every self-description; a v117 receiver ignores the unknown kind. The receiver turns each displayed picture (`beam_colour::rotate_rgb`, an index map); the self-view turns the same way and is MIRRORED for a front camera (a desktop webcam counts as one).

Layout (Nick: "keeps the image top as big as it can, so not letterbox but letterbottom… the wave buttons are in the empty space below the video"): the peer's picture fills the full width, top-anchored, down to just above the button stack (`wave_field::picture_region`); the self-view is an inset in the picture's bottom-right corner.

Once a picture is up the screen is picture + buttons: no avatars, ripples, name or delay labels; only a warning status (microphone, reconnecting) still draws over it (Nick 2026-10-14).

The swipe in (Nick 2026-10-14: "the swipe to start is really nice. I like not even starting with a keyframe. Nice and empty!"): blocks the decoder has never painted (OpenH264's zeroed buffers, Y = Cb = Cr = 0, which no encoder writes) travel as a quarter-size hole mask (`I420::unpainted_mask`, turned with the picture) and the blit leaves them untouched, so the speckle background shows thru until the intra-refresh stripe reaches them. The same happens for a moment after a band change.

## 5c. The rungs (decided + built 2026-10-13)

H.264 cannot predict across a coded-size change (a new size = new parameter sets + a keyframe); VP9, AV1 and VVC can (reference scaling / resampling), but none has hardware encode on the phones. So the CODED size changes rarely and the DETAIL moves instead: each camera frame is cropped to 16:9, box-shrunk to the effective size and grown back to the coded size before the encoder (`beam_scale.rs`) — a softer picture, fewer bits, no keyframe, any decoder.

Five coded bands, each double the last (Nick: "fill out two more rungs… 2560x1440 and 160x90"): 160×90, 320×180, 640×360, 1280×720, 2560×1440. Detail `d` in half-stops of effective width above 160; band `b` covers `d ∈ [2b−4, 2b]` (two stops); adjacent bands overlap by one stop and the ladder crosses a band only on LEAVING the current one's range, so a link teetering on an edge holds the lower band (Nick: "overlap at least one 'stop' per band so it doesn't jump back and forth… sticks in the lower"). Below the smallest band the lever is frame rate (free in H.264): 30 → 5 fps. A 1280×720 coded stream cannot be softened all the way down: every 16×16 block costs a few bits however smooth (`bitrate_for` carries that floor), which is why bands, not softness alone, carry the bottom.

Steering (`beam_ladder.rs`): the audio engine's once-a-second verdict on the shared path (`wave::LINK_VERDICT` — congested on corroborated loss or a building delay slope, clean on no loss and a calm slope): congested → down a half-stop at once; three clean in a row → up a half-stop. Ceilings: the camera's capture after the crop; 2560×1440 only on a LAN-fast path (desktops); phones stop at 1280×720; metered → 640×360. Bitrate moves live within a band (MediaCodec `PARAMETER_KEY_VIDEO_BITRATE`; OpenH264 `ENCODER_OPTION_BITRATE`); a band crossing restarts the encoder at the new size and re-describes the beam.

## 5e. Capture, lenses and the camera cycle (built 2026-10-14)

Nick: "sampling what we have available and working with it, even if we only get a 25fps camera with a 2:1 aspect… cap the framerate at 30 and the megapixels at 5 ish… ideally uncropped", and "tap and switch between the front and back cam, or the other three back cams… cycles thru all when I tap on my preview".

**The capture** (PhotonBeam.pickCapture). Of the YUV sizes the sensor streams (≤ ~5 MP), the pick is, in order:
1. The highest frame rate up to 30.
2. The least crop of the sensor's own aspect (within 1% counts as uncropped).
3. The most pixels.

On the Pixel 8 Pro every lens lands on 2560×1920 @ 30. The back sensors are 4:3 (4080×3072), so that is uncropped. The front sensor is 3440×2448 (1.405), and its only uncropped size is the full 8.4 MP frame, over the cap, so the front loses ~5% of its width.

**The bands** (beam_ladder::bands_for). The top band is the capture, and each band below is an exact half, at the capture's own aspect: 2560×1920 → 1280×960 → 640×480 → 320×240 → 160×120. Nothing is cropped to 16:9 any more. The ceiling (`ceiling`):
- On a LAN-fast path (≤ 15 ms round trip): the whole capture.
- Elsewhere: the highest band ≤ 1280 wide, because the far side's software decoder is the limit.
- On a metered network: two bands lower again.

Kotlin reads the coded size straight from the packed plan (`pack_plan`: width << 24, height << 40). No band table is duplicated.

**The lenses** (PhotonBeam.listTargets):
- Every logical camera counts.
- A logical multi-camera with two or more streamable lenses contributes each lens as its own target. It is opened through the logical id, with the stream pinned to the lens (`OutputConfiguration.setPhysicalCameraId`), so the picture and its colour matrix are that sensor's.
- A lens seen twice (same facing, focal length and sensor width) counts once.
- In-sensor crop modes stay as zoom stops (Nick: "you may want to zoom in").
- Order: front first, then the back lenses widest to longest. On the Pixel 8 Pro: front, ultrawide, main, main 2×, tele, tele 2×.
- A HAL that refuses a physical stream falls back to the logical camera.

**The switch.** A tap on the self-view inset calls `cycleBeamCamera`. The camera thread closes the session, reader and encoder, then:
1. Reopens the device, or only the session, between lenses of one logical camera.
2. Picks the capture.
3. Fires the pipeline-up edge again.

Rust's `android_started` sees a live sender and keeps it, because a new BeamTx would restart its key chain. It rebuilds the ladder from the new capture, keeping the link's detail, and re-describes the beam: size, fps, the lens's own colour entry or calibration, and orientation. The first planned frame builds the encoder at the new size, and its keyframe is the one the new size needs. Calibrations are kept per lens; a scan that finishes after a switch is kept for its own lens, never applied to the new one.

## 5d. Calibration with chameleon (built 2026-10-14)

Nick: "on the Chameleon integration, yes. However you want to wire it in."

**Wiring: a runtime plugin, never a Cargo dependency.** chameleon is closed source; photon is public. Even an optional path dependency behind a feature breaks every checkout without the private sibling (Cargo resolves optional path dependencies for the lockfile, measured 2026-10-14), so photon never links it. The Android publish builds chameleon's `cdylib` when `../chameleon` exists and packages `libchameleon.so` beside photon's library (scripts/lib/android.sh). Photon `dlopen`s it (platform/chameleon_plugin.rs) and shows **Calibrate** only when it is there. The one entry point is chameleon's `chameleon_scan_linear_rgb` (ffi.rs): linear RGB in; out come the scan's XYZ→camera ColorMatrix (D50, row-major) and the target's type, serial and timestamp.

**What a tap does** (wave/beam_calibrate.rs):
1. The next full-size camera frame is copied out of `nativeBeamFill` before the ladder shrinks it.
2. That frame goes back to linear camera RGB: the receive chain's 601 full-range decode, then γ2 squared away.
3. chameleon scans it on a worker thread.
4. The matrix becomes the beam's entry, by lumis's derivation: camera → VSF RGB = (XYZ → VSF RGB) × (XYZ → camera)⁻¹. It is labelled `chameleon` / `relative` / `unit`, illuminant 23, and carries `CalProvenance`.
5. The beam re-describes itself at once (`android_apply_calibration`), and the self-view switches with it.

The status line says "looking for the target…", then the outcome. The outcome stays up until the next poke.

**Only on the straight path.** With the camera's own AWB running, the next scene moves the colour out from under a one-frame matrix, so the button stays hidden. **The calibration lasts the app run, as in lumis:** a relative IDT is the camera under the light it was scanned in.

**Network:** the first scan of a target serial downloads that target's spectral data from VERICHROME; after that it is offline. chameleon encrypts that cached data at rest. Photon binds it to the DEVICE, not the install (Nick: "no sense in hiding cal data from others… so reinstalls still read the chart/colour data and so do other users"), by handing chameleon `spaghettify("verichrome-cache-v0" ‖ machine fingerprint)` in place of its random per-install salt. The derivation is one-way and domain-separated, so the closed library never sees the fingerprint. Any install, user or app on the device that derives the key this way reads the same cache. On Android, ANDROID_ID is per user profile and per signing key, and an uninstall deletes the files themselves, so there a reinstall fetches each target once more unless the files come back (a backup restore).

**Desktop:** not shipped yet. The loader looks beside the binary, and in a macOS bundle's Frameworks, but no installer carries the library, and a desktop camera's own AWB makes a one-frame matrix drift anyway.

**Found on the way: VSF matrices were read transposed.** `vsf::colour` constants are column-major; a `ProfileEntry` matrix and this chain are row-major. The beam read `SRGB2VSF_RGB`, `XYZ2VSF_RGB` and `VSF_RGB2REC2020` raw, so sRGB white came out as (0.65, 1.35, 1.00): every `assumed` beam (Samsung fallback, Mac) was green. Now every VSF constant enters the chain thru `beam_colour::vsf_rows`, and a grey-stays-grey test guards it.

## 5f. HDR beams (built 2026-10-26)

Nick 2026-10-26: HDR beams as VSF RGB g2t. A phone camera that offers 10-bit HLG (`REQUEST_AVAILABLE_CAPABILITIES_DYNAMIC_RANGE_TEN_BIT` + the `HLG10` profile + P010 output sizes, API 33) streams P010 into its reader when Settings → Beam asks for HDR; the request keeps the camera's own colour (the HLG10 profile carries the HAL's processing to BT.2020 — no straight path, no γ2 curve of ours). Per frame (`beam_hdr.rs`): the P010 planes box-shrink at 16 bits to the effective size, each pixel goes 10-bit BT.2020 Y'CbCr (video or full range, as the frame's dataspace says) → HLG R'G'B' → light (the inverse OETF, the OOTF's system gamma 1.2 per channel, HLG's reference white 0.75 = diffuse white, its top 4.9× white) → g2t code by one 4096-entry table, blue-noise dithered, re-encoded as the same 601 full-range 8-bit YCbCr the encoder always takes. The entry is `creative` / `assumed`, the Rec.2020→VSF-RGB matrix, transfer `g2t` (`hdr_entry`). An HDR beam is an ordinary H.264 stream; a receiver keeps its light past white (§4) and an HDR screen shows it (the Android extended-range surface asks for the frame's brightest light); an SDR screen clips each channel at white. Desktop cameras (V4L2, AVFoundation) are 8-bit SDR: HDR is greyed out on their Settings page. The self-view of an HDR camera reads the high byte of each P010 sample (HLG standing in for g2t's curve, the inset only).

## 5g. Settings (built 2026-10-26)

Settings → Beam (Nick 2026-10-26: "a beam settings screen to control bitrate/size/quality/HDRness… and test gradients… to check our HDR against 'white' and 'brightest' and see stairsteps for any banding/bitdepth"). The controls, all live (`beam_settings.rs`: one generation counter each sender reads on its frame edge):

- **HDR** off / on (default off; greyed out without a 10-bit HLG camera). Takes effect at the next camera start.
- **Largest size**: Auto (the link's ceiling, §5c) or 1920 / 1280 / 640 / 320 wide — a cap only ever lowers the ceiling.
- **Frame rate**: 30 / 24 / 15 — the ladder's top rate; below the smallest band it still halves per stop, floored at 5. Desktops keep frames evenly (24 of 30 keeps four in five).
- **Quality**: lean / standard / rich = 0.06 / 0.1 / 0.18 bit per effective pixel per frame (`Tuning::bpp_milli`).
- **Most data**: Auto or 4000 / 2000 / 1000 / 500 kbit/s — the ladder's top rung becomes the highest whose bitrate fits, so the cap shapes size and detail, never just starves the encoder.
- **When the connection is busy**: keep it smooth (shed detail first, the default) or keep it sharp (shed two stops of frame rate first, `FPS_FIRST`, then detail).

The keys (`beam.hdr`, `beam.size_cap`, `beam.fps_cap`, `beam.quality`, `beam.bitrate_cap`, `beam.keep_detail`) are per device by default; one choice on the page links all of them to the fleet ("this device only" / "follow my fleet"). A device that never chose reads its own values or the defaults, never another device's. A cap is a number, or `u0(false)` for Auto.

The page's test patterns are painted straight into the buffer as g2t codes: black → white (codes 0–194), white → brightest (194–255) with ticks at each whole stop above white and a marker at the light the screen allows now, three patches (white, the brightest allowed now, the top code), near black one code per band, and each Rec.2020 primary from black up past white. A line says what the screen does: HDR with its allowed and asked-for stops (the Android extended-range surface's grant and declaration, `hdr_surface::status`), HDR allowing none (the phone's HDR brightness setting or battery saver), or SDR.

## 6. Stages (build order)

1. **Codec substrate (desktop).** OpenH264 as a desktop-only dependency through the arch gate; encode + decode of I420 in software; intra refresh if the library has it, else periodic keyframes on the desktop side only. A round-trip test.
2. **The video track.** A second media kind beside the wave's audio and its recording-fill plane, built the way the fill plane is: its own packet magic, its own sequence space, its own StepChains from a domain-separated secret (`"PHOTON_WAVE_v1 beam video"`), demuxed by magic at the media sink before the audio engine sees it. One encoded frame = one fountain-code window, fragmented to MTU-sized symbols with repair symbols in proportion; its own rate rung under the loss loop. **Nothing is added to the offer/answer**: the beam describes itself in-band — a `beam info` packet (geometry, fps, codec, the VSF characterization entry) at start and once a second, like intra refresh for metadata, so a beam can start mid-wave and a late joiner needs nothing from signalling; the per-frame gains ride each frame's header.
3. **Phone capture + encode.** Camera2 with the request above → YUV_420_888 → MediaCodec H.264 (low latency, CBR, intra refresh, keyframe interval never) → NALs to Rust over JNI, the same registration shape as the AAudio bridge. CAMERA permission with the POST_NOTIFICATIONS request pattern.
4. **Receive + render.** Decode (MediaCodec on the phone, OpenH264 on the desktop) → the integer chain of §4 → blue-noise dither → blit into the wave square. Beam buttons go live.
5. **Desktop capture.** V4L2 on Linux (opsin's live path knows the way), AVFoundation on mac and Media Foundation on Windows as "assumed sRGB" sources.
6. **Later:** `beam.video` recording track; long-term reference frames; 10-bit HEVC only if a target scan proves 8-bit sensor-space colour loses something a beam can see; hardware desktop codecs.

Related: docs/waves.md (the ceremony, container, lifecycle), docs/lexicon.md (beam), docs/PT.md "The bitmapless transfer" (what a kept beam will ride), docs/retention.md.
