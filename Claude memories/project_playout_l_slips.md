---
name: project-playout-l-slips
description: "Wave playout (2026-09-28/29): L = 1-in-256 of arrival EVENTS + repair slack + output lead, no hardcoded timings; l slips one sample per zero-slope point toward L; connect sweep measures speaker→mic"
metadata:
  type: project
---

Capital L = target, lowercase l = actual (Nick 2026-09-28; he disowned the old silence-only rule in lock.md §7.1: "We said nothing about silence").
- **L (engine.rs `target_latency`)** = the 1-in-256 point over the last 256 arrival EVENTS (an event starts where the age rises over the previous arrival; a falling run behind it — a queue releasing — is the same event) + repair slack (2 windows) + output lead (`audio::output_ahead`, the callback→DAC lead). NO HARDCODED TIMINGS: the first arrival sets L.
- Dead variants (don't revive): cutoff = 2 × floor (ages carry the clock offset, so it isn't offset-invariant); cutoff = floor + min RTT (blind to sender capture jitter — far worse); the per-window rule (Theresa WAN: l = 229 ms, a ringey echo loop).
- **l** moves exactly one sample per eligible point (slope 0 or sign change): drop if l > L, repeat if l < L, uncapped, never chained at one sample, never a jump. Nothing but slips moves l, not even a peer re-anchor.
- Each side reports its l once a second in LINK_TAIL_V3; the UI shows rung · far-ear latency under each avatar.
- **Connect sweep (wave/sweep.rs):** a 1 s log sweep 10 Hz → 20 kHz at −1.5 stops mixed over the wave (never holds TX); the matched mic tee logs `WAVE: sweep — speaker→mic X ms` = `a`, so our echo lands at name + l_theirs + a. Hangup plays the reverse.
- Next: report `a` to the peer; RX-side seeded canceller + residual suppressor; a predicted-echo duck.
- Desktop cpal capture+output buffering (~70 ms) is the biggest latency item.

**Why:** a 2.2 s playout on a 10 ms path (2026-09-28) came from seeding L off a startup burst and letting it move only in silence.
**How to apply:** no fixed timings in playout; any new latency term enters L, never a jump in l. Related: [[project_aligner_windup]], [[project_incall_learner]], [[feedback_stops_not_db]].
