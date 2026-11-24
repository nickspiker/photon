---
name: project_ferros_media_latency
description: "Owner direction 2026-11-24: waves and beams on ferros (8a + Pro) built for near-zero latency end to end, no fallbacks, designed up front with ferros, benchmarked and iterated"
metadata:
  node_type: memory
  type: project
  originSessionId: 17b90e8e-8b1a-448e-8ac7-22b057e9a0d7
  modified: 2026-11-24T22:43:54.450Z
---

**Direction (owner, 2026-11-24):** "no fallbacks, but yes, needs built ... work toward the full meal deal"; "q removal. Less q's the better and smaller the buckets without getting stupid"; "design up front, try options, experiment, benchmark, continue, repeat"; "coordinate with the ferros agent and let's get this shit FFAASSTT".
- No fallbacks: no plaid-PCM-instead-of-Opus path, no "skip H.264". Opus and H.264 get built for ferros. Stubs are allowed only as steps toward the real thing.
- Camera frames go straight thru as absolute VSF RGB g2t, calibrated (the IDT), ZERO white balance, so the far end sees the same light with bright snappy highlights; the owner works camera specifics with me when we get there.
- Shared-memory audio rings are welcome ("we don't have to reload page tables upon context switches so I bet we can keep these buffers lean"; "I do like owning things!").

**Why:** typical video calls run 150-300 ms; ferros owns every layer (one clock for mic and speaker, command-mode panel, zero-copy camera view), so tens of ms is reachable if the queues are designed out up front rather than ripped out later.

**How to apply:** the design is docs/ferros-media.md (canonical once written); every stage hands off the smallest unit (2.5 ms audio, a strip of rows for video), every queue holds at most one, one hardware clock per side. Split: ferros owns the audio service rings, AoC periods, display late latch + partial update, scheduler priority/pinning, mic right, capture stream; Photon owns its ferros audio/camera backends, Opus + H.264 for the ferros target, per-frame send, jitter-sized playout, slice encode/decode. Measure every stage; numbers in the doc. Related: [[project_wave_canonical_container]], [[project_beams]], [[project_aligner_windup]].
