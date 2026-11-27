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

**Measured** (ferros's audio agent, the Pro, 2026-11-24; ferros forge 59abc9f6, docs/AUDIO.md "The bench and the sweep"):

| What | Measured |
|---|---|
| Speaker at the AoC, EP5 (deep buffer) | 10 ms blocks of 480 frames, whatever period it is told |
| Speaker at the AoC, EP1 (low latency) | 10 ms blocks too |
| Speaker at the AoC, EP0 (MMAP) | 1 ms blocks of 48 frames (a 1 ms floor), but silent so far: blocked on something Android's HAL sends from userspace |
| Mic period at the AoC | 1 ms works (whole ms only, so no 2.5 ms period; Photon's reader gathers periods into its frame); 32-bit capture works |
| Loopback, speaker ring to mic (sweep impulse response, ferros's queue included) | EP5 34.2–34.3 ms, EP1 35.0 ms, raw record 34.85 ms; ferros's own queue is 0.67 ms of it |
| AoC interrupt to the service thread, idle | 24–36 µs mean, 66–123 µs worst on the A715s; 47 µs mean pinned to A510 core 2; under load not yet measured |

- **The ~34 ms is inside the AoC**: it does not move with the play path, the record path or the capture period, so it is the AoC's speaker chain or the shared mic front end. MMAP on both sides (EP0 and the MMAP record ring) is the critical path for sound; the next step is an AoC control capture under rooted Android (slot b), the owner's call.
- A fix on the way: the service was starving EP5 (5.84 s of samples out in 7.29 s) by topping up in 256-frame periods; it now tops up in frames.
- The draft APIs fit the engine's seam as they stand: ferros_sys `audio::mic` on the MIC mailbox (one writer, many readers, i32 per channel, a monotonic position and a capture-time anchor) and `audio::Shared` on SYS_SHARE, forge e4781298 (WRITE/READ/WAKE_AT, the DAC anchor, underruns).
- **openh264 on ferros** needs no C++ runtime library (Android builds it -fno-rtti -fno-exceptions, C headers only, its one dynamic_cast an upcast): about 300 lines of Rust shims in ferros_rt (new/delete, __cxa_pure_virtual, static-init guards), ~25 pthread and 7 semaphore calls onto ferros threads and futexes, and stubbed time, vsnprintf, getauxval and sysconf, for 62.6k lines of C++ and 8.5k of arm64 asm. The shims are the owner's call.

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

## Where it stands (2026-11-26)

Built and compiling for aarch64-unknown-ferros; not yet run on a phone (none on USB today).
- **Sound out**: the engine's render thread writes the MEDIA stream through the audio service (a3666b1a); an incoming wave rings and the ringback plays, chirp's synthesis on the wave output path (b180fba4).
- **Network**: every named port opens a socket ring at bind (ip ops 15/16, ferros ee7bd434) and sends and receives only through it; a wave's packets and express signals ride CLASS_VOICE (EF), a beam's CLASS_VIDEO (AF41) (c95aba60, 8f028cfd). ip feeds rings while the identity is unlocked (ferros fact 83), the same gate as op 2, so the screen going dark at the ear changes nothing.
- **Beam in**: OpenH264 decodes on ferros through ferros's openh264-sys2 fork (std/photon's patch, ferros e10814c7); the receiver starts with every answered wave, so a phone that waves back to a beam shows the picture (b180fba4).
- **Sound in (next)**: the `mic` right (ferros_kist::MIC = 15, ferros 91c9386f) is the proof that opens the MIC mailbox, and opening it lights the microphone bead; the mailbox carries the last 64 ms as 1 ms periods, each with its frame position and capture anchor, the header the newest position. Photon reads from its last position to the newest, opens the view when the engine starts and closes it at hangup. Until then the render clock hands the engine silence.
- **Beam out**: waits on the camera talk with the owner (the develop, the capture right).

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
