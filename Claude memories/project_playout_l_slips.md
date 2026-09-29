---
name: project-playout-l-slips
description: 2026-09-28 DECIDED+BUILT (unpublished at write): receiver playout = L target (max age of the last 256 RECEIVED windows + 2-window repair slack, first arrival sets it, no hardcoded timings) and l actual following it ONE sample per zero-slope/sign-change point, uncapped, never a jump, never silence-gated; the lock.md §7.1 silence rule is superseded
metadata:
  type: project
---

Field 2026-09-28 (Nick↔Emma, v0.106.3): playout latency 106101 samples (2.2 s) for a whole wave with a ~10 ms path — the first packets came in a 2.4 s burst, L was seeded from them, and the old rule let L move only in silence (lock.md §7.1 as written) which Emma's loudspeaker noise floor never reached.
Nick's rules (he disowned the silence rule: "We said nothing about silence"): capital L = target, lowercase l = actual.
- L (engine.rs target_latency): over the last 256 RECEIVED windows, floor = the shortest age, cutoff = 2 × floor (later = LOST, ignored — Nick: "trust the floor, it'll balance out"; no clock-degraded guard, no minimum), L = the 1-in-256 point of the ages inside the cutoff + repair slack (2 windows, repair_queue). NO HARDCODED TIMINGS (Nick 2026-09-28): no 20/60 ms start ("we just keep the first latency we get"), no step size, no 1 s re-anchor rule — the first arrival sets L and talking snaps l in.
- l moves by exactly one sample per eligible point (slope 0 or sign change), drop if l > L, repeat if l < L, uncapped ("we don't know if they are UWB direct or over a geostationary link"), one slip per point (a repeat must not chain at the same sample — test caught 40 chained repeats).
- Nothing but slips moves l, even a peer re-anchor.
- Each side reports its l once a second in a 13-byte LINK_TAIL_V3 (older peers drop just that packet; repair covers it); UI shows under each avatar: that voice's rung · far-ear latency (≈ when the clock is degraded).
Also built in the same batch: orb = solid path colour during an Active wave (update_orb wave branch, orb_wave), field frozen when unwatched (desktop window_attended; Android foreground && DisplayListener display_on — proximity blank), field α = brightest channel (background shows thru), colour cache ring keyed by frame number.
Related: [[project_aligner_windup]], [[project_audio_picker_levels]], [[feedback_stops_not_db]].

**2026-09-28 FIELD FINDING (unfixed, needs Nick's call):**
- The cutoff = 2 × floor is NOT offset-invariant. The ages are receiver clock − sender's name, so floor = true_min + δ (the clock offset, ±tens of ms between phones on nunc).
- δ > 0 inflates the cutoff (and L); δ ≤ 0 collapses it to about floor, so L is too tight and frames arrive too late.
- LAN wave, RTT 10 ms: Nick l = 44 ms with 1192 too-late frames; Emma l = 55 ms with 127 too-late. Emma's phone also captures in 960-frame (20 ms) bursts (no fast path), which makes her arrivals bursty.
- Proposed offset-free form: cutoff = floor + min RTT (the RTT is exact).
**2026-09-28 23:41 — floor + min RTT (v0.107.5) is WORSE. Field wave, Emma phone ↔ Nick desktop, LAN, min RTT 3 ms:**
- Emma: 4441 too late, 5638 misses, loss 123/256.
- Desktop: 7121 too late, 7330 misses.
- Budget lines (per side):

| | TX capture→send | jitter margin | repair slack | output |
|---|---|---|---|---|
| Emma | 16.4 ms | 2.4 ms | 20 ms | 8 ms |
| Desktop | 36.4 ms (cpal) | 3.1 ms | 10 ms | 33.3 ms (cpal) |

- Cause: sender-side capture jitter (bursty capture, tens of ms) is invisible to RTT, so floor + RTT marks most frames lost/late. The old 2×floor was accidentally lenient.
- Pending Nick's call. Recommended: L = the 1-in-256 point over ALL received ages (no cutoff; one straggler per 256 is the allowance).
- Separately: desktop cpal capture+output buffering ≈ 70 ms is the biggest latency item.
**2026-09-29 — L = 1-in-256 of arrival EVENTS; a burst counts once.** An event starts where the age rises over the previous arrival; the falling run behind it (a queue releasing) is the same event.
- Why: Theresa WAN wave, RTT 81 ms with spikes to 300 ms. The per-window rule gave a 134 ms jitter margin, l = 229 ms, and a half-second echo loop ("ringey").
- The cutoff variants stay dead.
- Echo: the current duck is speaker gain × OUR mic level NOW, but our echo returns a loop later. Proposed a predicted-echo duck (our voiced frames' names + the peer's reported l); not built yet.
