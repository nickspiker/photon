# Waves and beams on ferros, at the lowest latency

The owner, 2026-11-24: "no fallbacks ... it really all needs built, tested and shipped so I'd work toward the full meal deal"; "q removal. Less q's the better and smaller the buckets without getting stupid"; "design up front, try options, experiment, benchmark, continue, repeat"; "Like I want this creepy fast".

This is the design for mic, speaker, camera and display on the Pixel 8a and 8 Pro under ferros, end to end with a far party.
Photon owns its side (its ferros backends, the codecs for the ferros target, what it sends and how it plays out); ferros owns the services, the kernel and the panel. Everything marked **proposed** is a request to ferros, not a fact.

## The rule

Latency is the sum of every buffer something waits in.
- Every stage hands on the smallest unit it can: 2.5–5 ms of audio, a strip of picture rows.
- Every queue holds at most one unit ahead of its consumer.
- One hardware clock per side drives everything on it (the AoC's sample clock for sound, the panel's TE for pictures); nothing pads against drift it could measure instead.
- Every stage is measured, not estimated, and the numbers land here.

## The chain today and the target

Mouth to ear, two ferros phones on one LAN. "Today" is the design as built (AUDIO.md, BEAM.md), not a measurement.

| Stage | Today | Target |
|---|---|---|
| Mic period at the AoC | 10 ms | 2.5 ms |
| Mic to Photon | no stream for kists | a shared ring, no copy, no call per frame |
| Encode (Opus CELT) | no Opus on ferros | 2.5 ms lookahead + the frame |
| Send | one datagram per `ip` call, one request a loop | a shared ring into `ip`, sent the instant it lands |
| Network (LAN) | | ~1 ms |
| Jitter buffer | (Photon's loop: 1–24 frames over the path floor) | as deep as the measured jitter, no deeper |
| Speaker: stream ring | up to 1024 frames (21.3 ms) | one period ahead |
| Speaker: queued at the AoC | two periods (10.7 ms) | one period |
| Mix period | 256 frames (5.3 ms) | 64–128 frames (1.3–2.7 ms), if the AoC takes it |
| **Mouth to ear** | ~40–50 ms by design | **~12–15 ms** |

Glass to glass (camera to the far panel): today nothing; target ~25–35 ms at 60 frames/s capture, against 150–300 ms for a typical video call.

## Sound

**One clock.** The AoC's sample clock is the only clock for sound on a phone. Photon's engine already names every sample by its position and stamps the true instant of capture and of the DAC (src/platform/audio.rs: `captured_frames`, named playout, `play_l`); on ferros those come straight from the AoC's period count, not from a callback's guess. Echo cancellation then subtracts exactly what the speaker played at exactly that instant: the reference ring Photon fills before the device (RENDER_REF) is sample-accurate here, which no other platform gives it.

**Speaker (proposed, ferros):**
- A playback ring shared with the kist, a region the kernel hands out like the display surfaces (AUDIO.md already names this: "a ring shared like the surfaces would drop the copy"). The kist writes samples and advances an index; no IPC call per write.
- The writer keeps one period ahead, not the ring's depth; the ring's size is capacity, not latency.
- One period queued at the AoC, not two.
- The period as short as the AoC accepts without underruns: try 128, then 64 frames. The mixer runs once a period either way.
- The ring sound (alerts) is the same path on the alert class.

**Mic (proposed, ferros):**
- A capture stream for kists: the same shared ring, read side (the MIC mailbox is already planned in ferros_mailbox).
- The AoC's capture period down from 10 ms to 2.5 ms, if it takes it.
- 24-bit or 32-bit capture if the AoC carries it (Photon's engine takes 24-bit: `CaptureQ` holds i32).
- A mic right of its own, asked, and the gain policy (a quiet room reads ~78 dB under full scale today). Today any kist holding `audio` can read the meters and record: that gap closes with the right.

**Photon's side:**
- `platform::audio::ferros`: a capture thread and a render thread on the shared rings, feeding the engine's queues as the Android JNI side does, with the AoC's sample count as the clock.
- Opus for the ferros target, built from libopus with ferros's C path (tools/std-build.sh), its encoder and decoder made with `opus_*_init` in memory Photon owns, so no malloc crosses into C.
- The engine's frame is 5 ms today (240 samples); try 2.5 ms (120) on ferros-to-ferros and LAN paths, where the packet rate (400/s) costs nothing that matters.
- Plaid (raw PCM, the wire contract's own rung) has no codec delay at all; on a LAN it is the fastest rung, measured against CELT, not a fallback.

**Scheduling (proposed, ferros).** The scheduler is FIFO with a 10 ms slice and no priorities: an audio thread can wait a whole slice behind anything sharing its core, which alone is bigger than the target. The audio threads (the service's, and a call's capture and render threads) each get a core of their own (`ferros_rt::thread::pin`) or a real-time class that preempts on its doorbell. Measured: wakeup latency from the AoC's mailbox to the thread running.

## Network

BEAM.md's transport test: each datagram is one call into `ip`, which serves one request a loop after polling the WiFi driver (~2.3 frames/s). Sound at 200–400 packets/s and a beam's slices need **(proposed, ferros)**:
- A send ring and a receive ring shared with `ip`, like the sound rings; a datagram goes out the moment it lands, and an arriving one wakes its reader.
- Photon sends each frame the instant it is encoded (no batching), repairs loss forward (the CELT rungs' FEC, RaptorQ for pictures), and never retransmits sound.
- The direct LAN path over any relay; Photon's path ladder already prefers it.

## Pictures

**Camera.** The camera's view is already zero copy (a ring of buffers mapped in place, CAMERA.md). A higher-rate sensor mode, binned where that is what it takes, so exposure and readout are ~16 ms, not 33. Frames go straight thru as absolute VSF RGB g2t, calibrated by the sensor's IDT, with **no white balance**, so the far end sees the same light with its bright highlights (the owner, 2026-11-24). The develop (demosaic + IDT + g2t) runs on the GPU, Lumis's code shared, not copied. Camera specifics are worked with the owner.

**Strips, not frames.** Each stage works on a strip of rows (a multiple of 16, H.264's macroblock row), never waiting for the whole picture:
camera rows → develop the strip → encode it as an H.264 slice → send the slice → decode the slice → write those rows to the panel.
The delay at each step is one strip, not one frame.

**H.264 for ferros** (the house beam codec): no B-frames, no lookahead, gradual intra refresh instead of keyframes (the bitrate stays flat, so no keyframe ever queues behind itself), a rate-control buffer under one frame. Encode and decode both have to exist on ferros: openh264 built with ferros's C path (it is C++, so the sysroot needs what that takes), or a Rust codec; tried and measured, both directions.

**Wire.** Today one access unit is one RaptorQ object (BEAM.md). Strips want one object per slice, so the far end decodes a strip as soon as its symbols land: a beam wire change for every platform, versioned and taken by Photon everywhere at once, not a ferros dialect.

## Display

The display is the solid link: a command-mode panel at 120 Hz, a server that answers a frame before drawing it and keeps only the newest, damage by 64×64 tile, `frame wait` on the panel's TE, measured at 118–120 frames/s on the Pro. Two steps **(proposed, ferros)**:
- **Late latch**: compose at the TE minus the compose-and-transfer budget, so the frame shown carries the newest strip, not one up to 8.3 ms old.
- **Partial update**: send the panel only the rows that changed (a beam's strip), shortening the transfer from present to light.

## Measuring

Every stage stamps the AoC sample count or the TE count it acted at, and the numbers go in the table above.
- **Mouth to ear**: Photon's chirps through the real path, one phone's speaker to the other's mic and back, correlated.
- **Loopback on one phone**: mic ring → engine → speaker ring, the device's own floor with no network.
- **Glass to glass**: the far phone's camera filming the near panel's frame counter, the two counts compared.
- **Wakeup**: mailbox doorbell to thread running, per core and class.

## The order

Each step is shipped, measured and kept; stubs only as steps toward the real thing.
0. This design, agreed with ferros.
1. Sound out: Photon's ferros render path on today's `WRITE` (to measure the floor), then on the shared ring when it lands; libopus for ferros, decode first.
2. Sound in: the capture ring and the mic right (ferros), Photon's capture path, Opus encode; a full wave ferros to anywhere.
3. Calls that feel right: the earpiece, the ring and the live note, a backgrounded Photon woken for a call (ferros); echo cancellation on the one clock.
4. The network rings (ferros): sound at 200–400 packets/s with no batching.
5. Beam in: H.264 decode on ferros, the newest picture shown, then strips and partial update.
6. Beam out: the develop on the GPU, H.264 encode by slices, the per-slice wire.
7. Tighten: shorter periods, late latch, pinned or real-time audio, 2.5 ms frames; measure after each.
