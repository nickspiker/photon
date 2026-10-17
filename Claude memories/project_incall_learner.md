---
name: project_incall_learner
description: "ECHO DOCTRINE since 2026-09-13: the mic is UNTOUCHED on TX (no AGC/canceller/duck); the speaker ducks on mic level right before the DAC, recorded nowhere; chirp + learner measure only"
metadata:
  type: project
---

**Doctrine (Nick 2026-09-13, hard):** "Mic needs untouched before it hits the wire, filtering is always done on the speaker side which is only temporary… if the mic level gets hot, drop the speaker volume right before it gets played, do not keep record of post filtered values."
Shipped 49fe774:
- TX = ADC → Opus → wire. No AGC, no NLMS, no mic duck. The wire copy is each party's good copy; a kept wave needs only fills ([[project_recording_fills]]).
- Speaker duck in `platform::audio::next_render_frame_at`: `gain = 1 − near / SPEAKER_DUCK_MIC_FULL` (the one knob) clamped [0,1], where `near` = mean |sample| of the newest captured frame; the chirp plays at full; a headset disarms it (`set_speaker_duck`).
- Chirp + learner still measure (g, delay, floor) for the log only.
History: NLMS field rounds 1–6 (2026-09-08) never converged.

**Why:** a TX-side gate silenced Nick whenever the far end made a sound, and even a linear mic duck put a ducked copy on the wire.
**How to apply:** never add processing to the capture path; tune `SPEAKER_DUCK_MIC_FULL` from the field ("speaker ducked N of M render frames"); any canceller runs on the listening side or offline on kept raw records ([[project_playout_l_slips]] holds the RX canceller / predicted-echo plan).
