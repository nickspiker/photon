---
name: project_aaudio_clockid_ignored
description: 2026-09-26 v104 CONVICTED: Pixel AAudio HALs answer getTimestamp on CLOCK_MONOTONIC even when asked for BOOTTIME — always request MONOTONIC and map to BOOTTIME with a back-to-back clock read
metadata:
  type: project
---

**Conviction (v104 field wave, Nick + Emma phones, 2026-09-26):** `AAudioStream_getTimestamp(CLOCK_BOOTTIME)` returned MONOTONIC-based times. MONOTONIC trails BOOTTIME by total suspend time (21 h on one phone, 55 h on the other), so LOCK capture stamps jumped by hours after the first frames (aligner phase −3.6e9 samples, "adc −1,045,869 ppm") and named playout's DAC instant sat hours before every frame name: all 3104 decoded frames "left waiting", 0 played — two waves connected with no audio either way.

**Fix (commit 990e6d1c):** `audio_aaudio::frame_time` asks for `Clockid::Monotonic` and adds `CLOCK_BOOTTIME − CLOCK_MONOTONIC` read back to back. Guards: the aligner re-anchors on a stamp >10 ms off its fit; the receiver resets its path floor / play position on second-scale jumps.

**How to apply:** never trust a platform API to honour a requested clock id — take the clock it actually uses and convert it yourself. Tell-tale in logs: `WAVE: lock —` with absurd adc ppm / residual in seconds, or `WAVE: playout — … N far frame(s) left waiting, 0 miss(es)`. Related: [[project_wave_canonical_container]], docs/lock.md.
