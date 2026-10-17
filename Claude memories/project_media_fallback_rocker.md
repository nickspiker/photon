---
name: project-media-fallback-rocker
description: "Android wave render: NO automatic media fallback (2026-09-29) — voice usage, fast by default, the earpiece a user choice; rocker/proximity follow the actual usage; BT output-stall watchdog"
metadata:
  type: project
---

Convicted 2026-09-26 (Emma's SM-N976V): voice usage lost the fast path, the render reopened on USAGE_MEDIA (volume 0.067) while Kotlin bound the rocker to STREAM_VOICE_CALL and armed earpiece proximity → a silent wave with the rocker maxed.
Current state:
- **No automatic fallback** (Nick 2026-09-29: "shouldn't be any fallback, that should be user choice what source and volume to run"). start_output opens voice-communication usage; fast → kept on every route; slow → loudspeaker/wired take MEDIA (fast), the earpiece stays voice only when the user picked it, Bluetooth keeps voice.
- **Fast by default, the earpiece a choice:** device-local `audio.route`; per-route + usage volume `audio.vol.<route>.<v|m>` restored after each output open; the route pill shows "Earpiece (slower)" on slow devices.
- PhotonConnectionService `atEar()` = earpieceRouted && renderVoiceUsage drives proximity + volumeControlStream; renderUsageVoice/Media re-run applyRouteSideEffects.
- **BT output stall** (2026-09-30, X15 earbuds): AAudio reported up but never called back; the engine.rs arrival-edge watchdog rebuilds (`audio::rebuild_streams()`) when 400 far frames queue with no render.
- The wave screen is its own screen: `PhotonApp::wave_screen()` gates conversation layout and input.

**Why:** a vendor that steals the fast path must never leave a user with a silent wave and no remedy.
**How to apply:** the rocker, proximity lock and route pill follow the render's ACTUAL usage, never the requested route. Related: [[project_waves]], [[project_playout_l_slips]].
