---
name: project-waves
description: "waves (voice) FIELD-WORKING since 2026-08-19: three planes, dozen-egg keys, CBR ladder, recording by default, Rust-owned AAudio exclusive on Android; docs/waves.md is the design"
metadata:
  type: project
---

`docs/waves.md` is the concrete design and dated decision log; this is the pointer + the load-bearing decisions. Code: `src/wave/`, `src/platform/audio*.rs`, `src/ui/photon_app/wave_ui.rs`. Built 2026-08-18 as "voice calls", renamed 2026-09-23 ([[project_wave_beam_transition]]).

- **Three planes:** SIGNALING rides friendship lanes as encrypted control rows (`WAVE_PREFIX`; wire-indistinguishable from text); MEDIA is an ephemeral UDP plane (non-VSF datagram, recv fast-path before the parse ladder); HISTORY = the wave card row + optional kept recording.
- **Keys = the dozen-egg basket, no lone ECDH** (a single X25519 would be the only quantum-soft link). The offer's lane key is doomed material → per-wave forward secrecy; intra-wave ratchet steps by packet COUNT, never a timer.
- **Codec:** Opus low-delay on a channel-aware CBR ladder (rungs by name, AIMD with hysteresis). VBR permanently off the table (packet-size side channel). No PLC; a hole is silence.
- **Ring/answer** = attention machinery inverted: ring bypasses suppression, first answer wins, no ring timer, ring requires direct decrypt.
- **Recording by default** (Nick: dislikes services that deny later note-taking): endpoint memory, never wire retention; Delete = drop the spool key ([[feedback_discard_not_shred]] for wording).
- **Android audio is Rust-owned AAudio** (`audio_aaudio.rs`, exclusive + low-latency, shared fallback), every frame stamped with ADC/DAC time ([[project_aaudio_clockid_ignored]]); Kotlin keeps proximity lock, FGS, permission prompt, route mirrors.
- **Echo doctrine:** mic untouched on TX, speaker ducks pre-DAC ([[project_incall_learner]]); vendor voice DSP optional ([[project_wave_fishtank_cellular]]).

Deferred v1 gaps (docs/waves.md "Explicitly deferred"): relay media, group waves, cold-wake ring via FCM, local transcription.
Related: [[project_wave_card]], [[project_wave_canonical_container]], [[edges-not-timers]].
