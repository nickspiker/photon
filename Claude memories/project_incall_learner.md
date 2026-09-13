---
name: project_incall_learner
description: "Echo doctrine since 2026-09-13 — the MIC IS UNTOUCHED on the TX path (no AGC/canceller/duck); the SPEAKER ducks on the mic level right before the DAC, recorded nowhere; chirp + learner measure only"
metadata: 
  node_type: memory
  type: project
  originSessionId: 87fd33b1-f610-46f9-b464-4fe0dd1e3280
  modified: 2026-09-13T18:13:14.007Z
---

**Doctrine (Nick 2026-09-13, hard):** "Mic needs untouched before it hits the wire, filtering is always done on the speaker side which is only temporary… if the mic level gets hot, drop the speaker volume right before it gets played, do not keep record of post filtered values."

**Shipped 49fe774 (Android dev 0.95.12):**
- TX path = ADC → Opus → wire. No PID AGC, no NLMS subtract, no mic duck. The wire copy is the good copy of every party; a kept wave needs only fills for missed windows (no party ever needs another's archive). Archive stream still spools the same frame with unity proc (a second encode of the wire copy) — flag day to drop it pending.
- Speaker duck in `platform::audio::next_render_frame_at`: `gain = 1 − near / SPEAKER_DUCK_MIC_FULL` (1500, the one knob) clamped [0,1], `near` = mean |sample| of the newest captured frame (`note_near_level` per frame; muted zeros = silence). No slew/floor/hold; chirp (local source) plays at full; headset disarms (`set_speaker_duck`); reset with `clear_queues`.
- Chirp + learner still measure (g, delay, floor) for the log only; `COUPLING_WARN` 0.3 logs a rocker warning at the seed.

**Why:** the TX-side gate silenced Nick whenever Brittany's end made a sound (c617a36f, g 6.1 at low volume, mic 67); the morning's linear mic duck fixed that but still put a ducked copy on the wire, which is not the good copy.

**How to apply:** never add processing to the capture path; tune `SPEAKER_DUCK_MIC_FULL` from the field (echo line: "speaker ducked N of M render frames"); a subband/offline canceller, if ever, runs on the listening side or on kept raw records. Related: [[project_wave_card]], [[project_attachments]].

History: learner + V-CHIRP shipped 2026-09-08; NLMS field rounds 1–6 never converged (−0.35 to −2 dB, disarmed every wave); 2×-TX double capture fixed 2026-09-08.
