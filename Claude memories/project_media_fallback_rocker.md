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
