---
name: project-audio-picker-levels
description: "2026-09-28: wave FIELD BUILT (ripples from avatars, age-0 colour rings, a fluor layer); independent mic + speaker pickers and level readouts DECIDED but unbuilt"
metadata:
  type: project
---

**Pickers (decided, unbuilt):** mic and speaker chosen independently, Bluetooth included; two lists each starting with Auto; a choice that forces the other side SAYS so (BT headset mic → headset output at HFP quality; earpiece → latency on fallback phones). No pre-grouped pairs.
Every input capturable at once shows live, others grey unless selected; NEVER rotate-probe (Nick: "rotating would add confusion").

**Level readout (decided, unbuilt):** TX = what the far side gets (post level plan), RX = before OS volume. Arabic mode = dBFS (Nick's explicit exception to [[feedback_stops_not_db]] for this label only), dozenal = linear fraction of full scale, hex = 0.xx. Colour linear in amplitude: green < ½ FS, yellow at ½, red at clip.

**Wave field BUILT 2026-09-28 (replaced bars):** a 24 ru square at the top of the Active wave screen; their avatar ⅓ in from top-right, ours ⅓ in from bottom-left.
Audio age from (d² − r²)/k in doubled integer coords (no sqrt), 8.8 fixed-point ages interpolated, precomputed per size (`wave_field::FieldMap`); reach = the other avatar's near edge = one second; brightness = amplitude 1:1 in linear light, fading linearly to 0 at the reach.
The field is a fluor LAYER (darkness premultiplied by α); chrome flattened first, speckle last; avatar rings = the field's age-0 colour; the path colour lives in the TL orb.
Data: `wave::live` rings fed by engine TX (post level plan) and RX (`queue_named`, ages from `audio::play_head()`).

**Android facts:** AAudio setDeviceId per stream allows independent in/out; usually one physical input per app; a BT mic forces HFP/SCO; earpiece only via communication-device routing; A2DP adds ~100–250 ms that phones may misreport.
Related: [[project_media_fallback_rocker]], [[project_level_plan_reaim]], [[edges-not-timers]].
