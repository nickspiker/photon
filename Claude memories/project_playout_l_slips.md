---
name: project-playout-l-slips
description: 2026-09-28 DECIDED+BUILT (unpublished at write): receiver playout = L target (1-in-256 late RECEIVED windows + 2-window repair slack, starts 20/60 ms) and l actual following it ONE sample per zero-slope/sign-change point, uncapped, never a jump, never silence-gated; the lock.md §7.1 silence rule is superseded
metadata:
  type: project
---

Field 2026-09-28 (Nick↔Emma, v0.106.3): playout latency 106101 samples (2.2 s) for a whole wave with a ~10 ms path — the first packets came in a 2.4 s burst, L was seeded from them, and the old rule let L move only in silence (lock.md §7.1 as written) which Emma's loudspeaker noise floor never reached.
Nick's rules (he disowned the silence rule: "We said nothing about silence"): capital L = target, lowercase l = actual.
- L = the 1/256 quantile of arrival ages of RECEIVED windows (late → L += 24·255/256, early → L −= 24/256, Q8 in engine.rs l_q8), + repair slack (2 windows — the repair copy already rides two behind, repair_queue); lost packets never move L (delay can't fix loss; the duplicate does). Starts at 20 ms LAN / 60 ms WAN.
- l moves by exactly one sample per eligible point (slope 0 or sign change), drop if l > L, repeat if l < L, uncapped ("we don't know if they are UWB direct or over a geostationary link"), one slip per point (a repeat must not chain at the same sample — test caught 40 chained repeats).
- A >1 s mismatch is a peer re-anchor, not latency: reading restarts.
- Each side reports its l once a second in a 13-byte LINK_TAIL_V3 (older peers drop just that packet; repair covers it); UI shows under each avatar: that voice's rung · far-ear latency (≈ when the clock is degraded).
Also built in the same batch: orb = solid path colour during an Active wave (update_orb wave branch, orb_wave), field frozen when unwatched (desktop window_attended; Android foreground && DisplayListener display_on — proximity blank), field α = brightest channel (background shows thru), colour cache ring keyed by frame number.
Related: [[project_aligner_windup]], [[project_audio_picker_levels]], [[feedback_stops_not_db]].
