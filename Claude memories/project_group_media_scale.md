---
name: project_group_media_scale
description: 2026-10-31 docs/group-media-scale.md — unlimited-size group waves/beams; steps 1a (party table), 1b (encode once) and the newcomer-only roll call BUILT (v136, field test pending); 2a+ next
metadata:
  type: project
---
Nick 2026-10-31 asked for waves and beams in molecules of unlimited size, then "do your best to incorporate our changes for molecules so I can get a thousand people in a meeting".
docs/group-media-scale.md designs four stages and an order of work (1a party table, 1b one group engine, 2a sender keys, 2b forwarders, 2c joined set from the forwarder, 2d signed seconds, 3a silence sends nothing + every voice, 3b speaker stage + subscriptions, 4 simulcast); its "A thousand people" section covers signalling and the screen at that size.
Nick's decisions 2026-10-31: signed seconds yes; relays automatic, on by default, "Relay for groups" device-local off switch; mesh while (N−1)×rate ≤ uplink/2; all voices; every audio channel recorded by default, video save+replicate OFF by default.

BUILT 2026-10-31 (v136 line, uncommitted to the field yet): 1a `platform::audio::PartyTable` + PARTIES replace every FAR_SLOTS array (also wave::SLOT_LINKS, beam PICTURES, live slots), claim_slot never fails; 1b `engine::GroupTx` — the lead link encodes once at the lowest wanted rung, every link sends the shared frames, windows end on the grid, `drop_from` makes a group drop step from the sent rung; roll call = express to the newcomer only, no lane row.
Still one engine thread per link (not the single group engine) and receive is per link — 2a sender keys + sender id in the header is the next real step, and needs a wire change.

**Why:** at a thousand, per-link engines and the N² roll call were the walls; the screen (face_tiles) is not the limit until 3b.
**How to apply:** field-test a 3+ person group wave (lead hand-off: hang up the first-joined link) before building 2a; open questions to Nick: mute-on-join past a size, relay only when no direct path (express currently relays every signal to every device). Related: [[project_group_waves]].
