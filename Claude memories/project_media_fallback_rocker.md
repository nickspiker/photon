---
name: project-media-fallback-rocker
description: "2026-09-26 CONVICTED (v0.104.2, Emma's SM-N976V): voice usage lost the fast path → render reopened on USAGE_MEDIA (STREAM_MUSIC at 0.067) while Kotlin still bound the rocker to STREAM_VOICE_CALL and armed earpiece proximity → she heard nothing with the rocker maxed; fix unbuilt"
metadata:
  node_type: memory
  type: project
  originSessionId: 87fd33b1-f610-46f9-b464-4fe0dd1e3280
  modified: 2026-09-26T16:13:29.523Z
---

Field 2026-09-26 15:57 wave Nick→Emma: Nick's side heard Emma; Emma heard nothing although she maxed the rocker.
Transport and playout were clean on both sides (≈3.8k/4.1k packets each way, 0 lost, Emma's rx(play) level 1360, 1 underrun per window).
Chain: audio_aaudio.rs start_output THE GUARD saw voice usage come up Shared/None (960 fr bursts) → "reopening on media usage, loudspeaker" → native calls renderUsageMedia().
Kotlin PhotonConnectionService: pushVolumeMirror correctly reads STREAM_MUSIC (logged volume 0.067 both windows, never moved), BUT applyRouteSideEffects binds `volumeControlStream = STREAM_VOICE_CALL` whenever earpieceRouted, ignoring renderVoiceUsage; renderUsageMedia() re-pushes the mirror only, not the side effects.
So the rocker raised VOICE_CALL (which nothing plays on) and the media render stayed at ~7%; proximity lock also armed (earpiece=true) so she held a blanked phone to her ear while the render went to the loudspeaker.
**Why:** a device whose vendor voice pipeline steals the fast path gets a silent wave with no user remedy.
**How to apply:** the rocker binding, the proximity lock and the route pill must follow the render's ACTUAL usage (renderVoiceUsage), not the requested route; renderUsageMedia/Voice must re-run applyRouteSideEffects. Related: [[project_waves]], [[project_level_plan_reaim]].

**FIXED 2026-09-28 (dev android after v106):** PhotonConnectionService atEar() = earpieceRouted && renderVoiceUsage drives proximity + volumeControlStream (VOICE_CALL at the ear, MUSIC for a media-usage wave, default otherwise); renderUsageVoice/Media re-run applyRouteSideEffects; the route mirror ignores the communication device for a media render. Field verify pending on a fallback device (Emma's SM-N976V).
**2026-09-29 — THE FALLBACK IS GONE (Nick: "shouldn't be any fallback, that should be user choice what source and volume to run"):** the render always opens with voice-communication usage; a device that denies it the fast path keeps it, logged. The voice-call volume governs it on every route (mirror + rocker). Why: Emma's phone played a whole wave on media at index 0, so her connect sweep read 'clean' and she could barely hear.
**2026-09-29 — FAST BY DEFAULT, THE EARPIECE A CHOICE (Nick: choosing 8 ms vs 220 ms is a choice, not an automatic switch, and fast should be the default).**
- start_output opens voice usage.
  - Fast → keep it on every route.
  - Slow → loudspeaker or wired take MEDIA (fast); the earpiece stays voice only when the user picked it (a default earpiece start moves to the loudspeaker via routeWaveSpeaker); Bluetooth keeps voice.
- The user's route pick is device-local `audio.route`; per-route + usage volume is `audio.vol.<route>.<v|m>`, restored after each output open.
- Route pill picks call nativeRoutePicked → rebuild. The route pill shows "Earpiece (slower)" on slow devices.
**2026-09-30 — BT OUTPUT STALL CONVICTED (Nick, X15 earbuds, Emma's wave):**
- After route churn (Disconnected rebuilds around the answer), the AAudio output reported "out up" on bt:X15 but was never called back: render frames frozen at 164 for 40 s, and again after re-picking the X15. Emma's frames piled up (25791 waiting); the mic sent zeros.
- Fix: an arrival-edge watchdog in engine.rs. 400 far frames queued with no render → audio::rebuild_streams().
- The wave screen is now a distinct screen: PhotonApp::wave_screen() gates conversation layout, input (keys, IME, taps, drops, scroll), springs, the chrome title and the bg scroll.
