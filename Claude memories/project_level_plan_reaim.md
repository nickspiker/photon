---
name: project_level_plan_reaim
description: Level plan since 2026-09-15 — quiet-anchored relative voiced gate, the bounded in-call RE-AIM (4 s speech-like evidence, dynamics gate, 24-coarse first-step floor, one correction), fine floor seeds above the vendor; jitter slack 80 ms + setpoint 1/512; audio session ownership + Android HANDOVER lock
metadata:
  type: project
---

State of the wave level plan and jitter after the 2026-09-14/15 field week (docs/calls.md is canonical for the plan; this is the tuning ledger).

**Level plan (engine.rs TX block):**
- Makeup seed priority: stored voiced → **stored fine floor × 20** ("normalize on quiet", Nick) → vendor sensitivity (sign-flip arm) → default. Initial makeup capped 16× (a stale profile can't crackle); the in-call re-aim may go to 64×.
- Frame means kept at 24-bit (q8 = coarse×256). `noise_est_q8` room tracker (slow rise >>10, fast fall >>2); voiced gate = mean > 3× tracker (and > 2 coarse); far-quiet gate emitted_level()<128 unchanged.
- **RE-AIM**: at most 2 steps/call. First: ≥800 voiced frames, dynamics gate p90 ≥ 2.5×p50 (flat = breath/room → discard, wait), first-step plausibility floor ≥24 coarse, step only if ≥1.5× off. Correction: ≥1600 fresh frames, ≥2× off, final. Field: 2026-09-15 18:31 Nick's wave needed no step (16× vs ideal 12.8×) — the profiles are converging (Nick 76–91, Emma 53–60).
- Teardown blend: RE-SEED when ≥2× off the store; count cap 40; a talkless call posts floor-only (voiced 0 sentinel).

**Why manufacturer calibration is untrusted** (Nick asked 2026-09-15): sign conventions flip between vendors (+37 vs −37 dBFS), plausible numbers miss by ~10 dB (input-gain topology unknown), PROPERTY_SUPPORT_AUDIO_SOURCE_UNPROCESSED is a self-declared checkbox, and getStreamVolumeDb's vendor curves reported −32 dB on an audibly-fine phone (volume mirror now = plain index ratio; Rust reports the render usage voice/media to Kotlin).

**Jitter (audio.rs / engine.rs):** STALL_SLACK 16 frames (80 ms; at 4 the guard fought the loss loop on bursty Wi-Fi: 50 underruns + 910 trims per minute); stall shedding is pause-aware (frames < RENDER_LEVEL/6 drop freely, voiced ≤1 per 8 renders); LOSS_SETPOINT 1/512 (the loop HELD 1/256 exactly = 50 audible gaps/min at plaid). Known limit: PLAYBACK_Q_MAX 240 (1.2 s) — a stall longer than that overflows at the front door before any guard runs; the fix is the radio, not the buffer.

**Session ownership:** `audio::start_owned()`/`stop_owned(gen)` — a stale owner's stop is a no-op (the ringback thread finishes its probe after the answer and used to tear down the engine's session: 0 packets out). AAudio stop holds SESSION thru the close; input open retries 3× on Disconnected. Android `HANDOVER` mutex makes start/stop whole units incl. the Kotlin chores (Emma's Note 10 screen-not-blanking: stopCallAudio landed after startCallAudio returned early).

**Presence:** opening a conversation arms a 1 s verdict on its ping (`presence_probe`, `presence_probe_tick`) — no pong → header offline now; a later pong flips it back.

**Discipline learned 2026-09-15:** another session shares this working tree (groups) — never `git add -A`; add named files. The publish script stamps its own version commit now.

Related: [[project_incall_learner]], [[project_voice_calls]], [[project_call_no_ring_incident]].
