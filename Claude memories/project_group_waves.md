---
name: project_group_waves
description: "2026-10-25/26 group waves + beams BUILT (mesh of 1:1 engines on far slots, beam fan, roll screen, recorded, per-link reconnect); field test pending"
metadata:
  node_type: memory
  type: project
  originSessionId: 7eb8c055-8c8c-4ba5-a64c-3d6cf003064c
  modified: 2026-10-26T04:38:47.356Z
---

docs/group-waves.md is canonical. Mesh: every joined device pair runs a whole 1:1 engine on its own FAR SLOT (platform::audio FAR_SLOTS = 8; slot 0 = the 1:1 wave, links claim 1..7 with a generation). Render sums live slots (named_mix); capture fans to every live slot.
Signals = 1:1 WaveSignal vocabulary on the molecule's lane: Offer = origin joins, Answer = member joins (+ roll call re-announce), Hangup = leave; signing device says who. No express copy. Ring lease = presence offline verdict (on_group_wave_device_offline).
Link secret = derive_group_link_secret(offer egg, wave id, both devices + nonces); lower device = origin direction.
Controller: src/ui/photon_app/group_wave.rs; answer/decline/hangup/reject/toggle_beam delegate to it. Lead link does session-wide work; only the lead writes the archive stream.
Beams: one beam receiver per link (beam.rs RxShared, pictures per slot); GROUP_FAN of BeamTx per link, beam_session::Target::{Pair,Group}. Screen: group roll (wave_field::roll_positions, env_ring_colour), picture_tiles grid.
Recording: per-link spools → record::finalize_group / merge_group_records (ch0 = our archive, ch1.. = far parties in link order).
Reconnect: group_drought_tick (2 s), wave::set_slot_redirect, slot_tx_addr.

Caught before shipping (ed4500e4): engine's far-slot var was `slot`, shadowed by the window-frame loop index → frames went to wrong slots; renamed `far`. Lesson: never blind-sed a variable into a loop body.
Also 339ca944: packet::open advanced the step chain BEFORE the tag check (any forged far-ahead seq killed a live wave).

**Why:** Nick 2026-10-25 "Needs built so wherever you want to start!" / "Go as far as you can with waves and beams" — one-model doctrine ([[feedback_any_size_conversation]]).
**How to apply:** field round = 3 people in a molecule: watch "group link up … slot N", per-link "engine down" tallies, "group recording kept", the roll rings lighting per speaker. Gaps: wave.env exchange for group cards, crash recovery of group spools, relay media, mesh limit 7.
