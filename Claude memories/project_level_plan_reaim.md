---
name: project_level_plan_reaim
description: "Wave level-plan tuning ledger (2026-09-15): quiet-anchored voiced gate, bounded in-wave RE-AIM, vendor calibration untrusted, jitter slack 80 ms, audio session ownership + Android HANDOVER lock"
metadata:
  type: project
---

docs/waves.md is canonical for the plan; this is the tuning ledger from the 2026-09-14/15 field week.
- **Makeup seed priority (engine.rs TX block):** stored voiced → stored fine floor × 20 ("normalize on quiet", Nick) → vendor sensitivity → default; initial makeup capped 16×, the re-aim may reach 64×.
- Voiced gate = frame mean > 3× the `noise_est_q8` room tracker (slow rise, fast fall).
- **RE-AIM:** at most 2 steps per wave. First: ≥800 voiced frames, dynamics gate p90 ≥ 2.5×p50 (flat = breath/room → wait), plausibility floor ≥24 coarse, step only if ≥1.5× off. Correction: ≥1600 fresh frames, ≥2× off, final. Teardown re-seeds the store when ≥2× off.
- **Manufacturer calibration is untrusted:** sign conventions flip between vendors, numbers miss by ~10 dB, UNPROCESSED support is self-declared, getStreamVolumeDb curves lie — the volume mirror is a plain index ratio.
- **Jitter:** STALL_SLACK 16 frames (80 ms); pause-aware stall shedding; LOSS_SETPOINT 1/512. PLAYBACK_Q_MAX 240 (1.2 s) — a longer stall overflows before any guard runs; the fix is the radio, not the buffer.
- **Session ownership:** `audio::start_owned()` / `stop_owned(gen)` — a stale owner's stop is a no-op (the ringback thread used to tear down the engine's session). The Android `HANDOVER` mutex makes start/stop whole units including the Kotlin chores.

**Why:** each level and jitter rule here was convicted by a field wave.
**How to apply:** change a constant only with a field log showing why; playout L/l rules live in [[project_playout_l_slips]], echo doctrine in [[project_incall_learner]].

**2026-10-29 — HOT-MIC GUARD (Nick: "It sounded 10x loud with a hard limiter on it … that should instantly fix itself down to 1:1"):** Josh's Linux mic ran raw ~4000 under the 8× opening makeup (no stored profile → cal voiced 78 default) and the re-aim never fired in a whole wave — its gate (frame > 3× the tracked room AND our speaker quiet) cannot see a mic whose level IS its room level, so the wire sat at 22–26k against 2048 into the cubic rail. Now `hot_mic_makeup` (engine.rs) runs on the 1 s cadence over every live frame's raw mean while our speaker is quiet: if raw × makeup would put > 2× TX_WIRE_TARGET on the wire, the makeup drops at once to the gain that lands the target, floored at 1:1 (the guard removes boost, never attenuates). Logs `WAVE: level plan hot mic — …`; the step rides reaim_history.
