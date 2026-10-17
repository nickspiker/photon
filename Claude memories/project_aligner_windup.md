---
name: project-aligner-windup
description: "2026-09-27 CONVICTED, FIXED 2026-09-28: TX aligner PI integrator wound up while slip rate saturated (1 slip/100 ms tick) → phase ran to −4016 samples; integ/acc now clamped + one-step realign"
metadata:
  type: project
---

src/wave/align.rs `Aligner::push`: `integ += filt*KI; acc += filt*KP + integ;` with at most ONE slip per 100 ms tick and no clamp. Field (v0.105.0, Theresa's phone, 17.6 min wave with Nick): unconverged first fits put the first name +374 samples off; the loop saturated, `integ` wound up, and phase overshot to −4016 samples (−84 ms) and kept going while adc read a sane −8 ppm. Receiver effect: drifting names moved the age floor under named playout → ~3000 underruns on Nick's side.

**Fix (2026-09-28, after v106):** integ clamped to ±1 slip/tick, acc to ±(1 + DEADBAND), and |phase| > FRAME (240 samples) re-aligns names in one step (`AlignStats.realigns`, logged beside re-anchors). Test `a_large_start_offset_never_winds_the_loop_up` (old loop: 478 samples / 889 slips; new < 0.2).

**Why:** any start offset beyond a few seconds of max slipping triggers windup, and unconverged first fits make that common.
**How to apply:** any rate-limited control loop needs anti-windup on every accumulator. Slips already land at flat points (least |x[n]−x[n−1]| + ¼|x[n]|), not zero crossings. Related: [[project_waves]], [[project_playout_l_slips]].
