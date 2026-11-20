---
name: feedback_sweeps_5_stops
description: HARD: every audio sweep (connect/end sweep, any calibration sweep) plays 5 stops under full scale; it had been left at 1.5 and was louder than the wave
metadata:
  type: feedback
---

Owner, 2026-11-20: "5 stops under is what I requested for all sweeps. Wondered why I thought I was going crazy."
The connect sweep (src/wave/sweep.rs `PEAK`) sat at 1.5 stops under full scale and is mixed by `play_overlay` without the wave's 4-stop output pad, so it played ~2.5 stops LOUDER than the voices it overlays, at every wave/beam start and end.
**Why:** the owner asked for 5 stops; it was never applied, and he heard it every call.
**How to apply:** any sweep or probe played into a wave peaks at `(1 << 10) as f64` (2^15 >> 5) or lower; the fit is a ratio with ~43 dB of processing gain, so level is not needed for measurement. When adding any overlay sound, compare its level against wave::OUTPUT_PAD_STOPS (4) and CHIRP_PAD_STOPS (3); an overlay skips the output pad. The unused V-chirp (src/wave/vchirp.rs) still has the old 1.5-stop PEAK_TARGET: fix it (and retune its test noise) if it is ever revived.
