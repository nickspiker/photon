---
name: project_wave_fishtank_cellular
description: "2026-10-02/03 wave sound arc: cellular fishtank = the 1-in-256 burst-once L rule (Nick's call); followers/ramp, re-anchor, render trace, recv-loop guard, duck depth, expander knee, paced walk, vendor voice DSP + Mac VPIO BUILT"
metadata:
  type: project
---

Every finding and fix below has its own dated section in docs/waves.md (2026-10-02 → 10-03); read that for detail.

**Fishtank on cellular (Jeff/Nick, Verizon, 0.110.3):** link clean (0 loss, 0 xruns) yet ~2% of 5 ms render frames holed. Cause = `target_latency` (wave/engine.rs): the 1-in-256 point with a burst counting once keeps L at the path, so each cellular queue spike's run of frames misses. Changing it is NICK'S CALL (he set it 2026-09-28/29; [[project_playout_l_slips]]).

**Convicted + fixed in the arc:**
- Underwater: per-5 ms gain staircase → `platform::audio::follow` followers + per-sample ramp; expander knee = 2 × far-room floor; loss term from the sweep coupling.
- 2x speed / warble: clock-step `names_stepped` → `reanchor_playout()`; L window = 1024 frames; walk paced at 1 slip per frame over audio, `WALK_DEAD_BAND` 1 ms; |l−L| > 200 ms or a DAC gap re-anchors.
- Receive loop away 1.3-2.8 s: `relay_reply` awaited inline in the recv loop → now spawned.
- Duck chop on clean routes: duck depth = sweep coupling / (1/32) (`DUCK_DEPTH_Q32`); opening makeup capped (`TX_MAKEUP_OPENING_CAP`).
- Instrument: dev builds trace every render frame (`RenderTrace` → spool RENDER_CHAN, `WAVE: trace #n` lines; `profile_enabled` = patch != 0).
- Vendor voice processing: `waves.voice_dsp` + `voice_dsp_wanted` (Android: checkbox or loudspeaker route); Mac VPIO in `src/platform/audio_vpio.rs`, macOS default ON; `VOICE_DSP_ACTIVE` (level plan) vs `VOICE_DSP_CANCELS` (duck/loss). Nick 2026-10-03: OS AEC "basically perfect", our loudspeaker path "legit unusable", raw earpiece = best sound + lowest latency.

**Open:** echo on loudspeaker without vendor DSP (a canceller touches the mic — doctrine forbids, Nick's call); per-route calibration; level-plan quiet anchor reads 0.00 on the Pixel earpiece; Emma's negative output lead after route rebuilds; post-wave chirp from a sibling-page card row (no gate for a wave this device took part in).
Related: [[project_level_plan_reaim]], [[project_incall_learner]].
