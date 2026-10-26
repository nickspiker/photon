---
name: project_group_waves
description: "2026-10-25 group waves v1 BUILT (b3b57611, 6699c780): mesh of 1:1 engines on far slots; gaps recording/beams/drought heal; field test pending"
metadata:
  node_type: memory
  type: project
  originSessionId: 09cee175-fdfa-4385-b01a-117eda364782
  modified: 2026-10-26T04:09:50.628Z
---

docs/group-waves.md is canonical. v1 = audio only, a mesh: every joined device pair runs a whole 1:1 engine on its own FAR SLOT (platform::audio FAR_SLOTS = 8; slot 0 = the 1:1 wave, links claim 1..7 with a generation). The render sums live slots (named_mix); capture fans to every live slot.
Signals = the 1:1 WaveSignal vocabulary on the molecule's lane (send_control_in_conversation): Offer = origin joins, Answer = member joins (+ roll call re-announce on each new join), Hangup = leave; the frame's signing device says who. No express copy for groups.
Link secret = derive_group_link_secret(offer egg, wave id, both devices + nonces, ordered by device); lower device key = origin direction.
Group controller: src/ui/photon_app/group_wave.rs; answer_wave/decline_wave/hangup_wave delegate to it, so buttons + Android notification Answer just work.
Lead link does session-wide work (sweep, duck, route, field TX, stall watchdog, readouts, calibration post); EngineParams.group = Some(GroupLink { lead, slot_gen }).

Found on the way, fixed separately (339ca944): packet::open advanced the step chain BEFORE the tag check — any forged far-ahead seq killed a live 1:1 wave. Now verify on StepChain::key_at copy, look-ahead capped 2^16 steps.

**Why:** Nick 2026-10-25 "Needs built so wherever you want to start!" — one-model doctrine ([[feedback_any_size_conversation]]).
**How to apply:** next field round = a 3-person umbrella wave; watch "group link up … slot N", "rx tally" open-drops (foreign links' packets — expected, harmless), and that all three hear each other. Next builds: N-party recording (one spool per party across links), group beams, drought heal for links. Not yet deployed as of writing.
