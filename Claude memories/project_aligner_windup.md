---
name: project-aligner-windup
description: "2026-09-27 CONVICTED (v0.105.0, Theresa's phone, 17.6 min wave with Nick): TX aligner PI integrator winds up while the slip rate is saturated at 1 per 100 ms tick → phase ran from +374 to −4016 samples (−84 ms) and kept going; fix (anti-windup clamp on integ + acc) unbuilt"
metadata:
  node_type: memory
  type: project
  originSessionId: 87fd33b1-f610-46f9-b464-4fe0dd1e3280
  modified: 2026-09-27T23:02:01.809Z
---

src/wave/align.rs Aligner::push: `integ += filt*KI; acc += filt*KP + integ;` with at most ONE slip per 100 ms tick (10 samples/s ceiling) and no clamp on `integ` or `acc`.
Field: Theresa's first fits were unconverged (first window adc −394 ppm, residual 1983 µs), so the first frame's name sat +374 samples (7.8 ms) off; the loop saturated inserting 10/s for ~37 s, `integ` wound up, and after phase crossed zero it kept inserting at the ceiling → −758, reversed, and by the wave's end it was −4016 samples held and still inserting (slips +7780 −2714), while adc read a sane −8 ppm.
Nick's phone on the same wave held ±1 sample (small start offset, never saturated) — the loop is fine unless it saturates.
Receiver effect: names drifting ~0.2 ms/s keep moving the age floor under named playout → steady underruns on Nick's side (≈3000+ over the wave).
**Why:** any start offset larger than a few seconds of max slipping (≈ >30 samples) triggers it; unconverged first fits make that common.
**How to apply:** clamp the integrator so its contribution never exceeds the deliverable rate (±1 sample/tick), clamp `acc` to about ±(1 + DEADBAND), and consider re-padding (re-anchor) when |phase| exceeds what slipping can close in a few seconds. Add a test that starts 400 samples off and asserts phase stays bounded. Related: [[project_wave_canonical_container]], [[project_waves]].
Also seen same wave (unconvicted): Theresa's RECEIVE lost 70–77 frames once a minute (rtt max ~1.6 s) right after her minute status ping + relay delivery to Nick's desktop.
