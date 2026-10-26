# Group waves

A wave in a molecule, at any size: the same wave as two people have (docs/waves.md), held by everyone who joins it.
Nick, 2026-10-25: "Needs built so wherever you want to start!"

**Status:** v1 BUILT 2026-10-25 — audio, mesh, no recording, no beams.

## The shape

A group wave is three things, each the 1:1 wave's own design carried to N:

1. **Signalling rides the group's lane.** The 1:1 vocabulary, unchanged: the `Offer` is the origin's join, an `Answer` is a member's join, a `Hangup` is a member leaving. They are hidden control rows (`RowControl::Wave`) in the molecule's conversation, sent by `send_control_in_conversation` to every standing member's device. The frame's signing device says who joined or left; nothing new is on the wire. There is no express copy: the group's lane is the only signal path (v1).
2. **Media is a mesh of 1:1 engines.** Every pair of joined devices runs its own media plane, a whole wave engine (keys, ladder, loss loop, fills, the link tail) exactly as a 1:1 wave runs one. The engines share the one audio session: each owns a FAR SLOT (`platform::audio::FAR_SLOTS`, 8), with its own copy of the microphone and its own far channel, and the speaker mixes the slots.
3. **History is one row per wave per device**, stamped `offer_osc + 1` in the molecule's conversation, outcome typed like a pair's (answered, missed, declined). Every device that saw the offer mints its own; its siblings fold it by stamp.

## Keys

Each link's secret is `derive_group_link_secret`: the offer's lane key (the doomed egg — the origin captures it at the send commit, every member at decrypt, exactly as a 1:1 wave does), the wave id, and both devices with their join nonces, ordered by device key so both ends derive the same secret.
The context string is its own (`PHOTON_WAVE_v1 group link`), so no group link can share a secret with any 1:1 wave.
The lower device key plays the origin's direction (`group_link_origin`), so the two ends' step chains never share a keystream.

## Joining and leaving

- The origin's offer goes out; its own record exists before the send so the commit can capture the egg.
- Every member device that decrypts the offer directly rings (the same `ring_alert`; ring on direct decrypt only, a stale offer over a minute old never rings).
- Joining sends an `Answer` to every member and brings up a link to every device already in. A member already in brings up its link to us when our `Answer` lands.
- Leaving sends a `Hangup`; every member takes its link to us down. The last device to leave ends the wave; the origin waiting for its first join waits on.
- Declining sends nothing — the others' wave goes on without us.
- A sibling of ours joining stops our ring: its `Answer` row reaches us by the fleet push and the molecule page merge.
- Glare: two members waving the same group in the same breath — the smaller wave id is the wave, and the other origin folds its offer into joining it.
- A device is in one wave at a time: a pair's offer during a group wave gets `Busy`, a group offer during any wave is ignored.

## The audio session

The group wave opens the session when its first link comes up and closes it when the wave ends; the links never open or close it (`EngineParams::group`).
One link is the LEAD: it does the session-wide work — the connect sweep, the speaker duck and route watch, the live field's TX side, the stall watchdog, the wave panel's readouts, the calibration post. When the lead's peer leaves, the lead passes to the next link.
Each slot has its own named playout: its own L, its own cursor, its own misses. The render sums the live slots (`named_mix`); one slot is that slot's frame exactly, so every 1:1 wave plays as before.
A slot is claimed with a generation (`claim_slot` / `release_slot`), so a link that ends and a link that reuses its slot never tear each other down.

## Ingress

Each engine installs its own media sink. A datagram goes to the sink whose engine already hears that address; one from an address no engine knows goes to every sink, and each engine's own seal keeps or refuses it — so a link whose peer moved finds it again by the same address-follows-auth rule as a 1:1 wave.
That broadcast is only safe because `packet::open` checks the tag before it moves the step chain (2026-10-25): it used to advance first, which let any datagram with a far-ahead sequence number ratchet a live wave past its own keys.

## v1 gaps

- **No recording.** The links run with no spool; a group wave's row has no card. The N-channel container (docs/waves.md, "One spool per identity") already names a channel per party — the keep needs one spool per party across the links.
- **No beams.** A beam in a mesh is N−1 uploads; it waits for a ladder that sizes to the weakest receiver, or a member relaying.
- **No drought heal.** A 1:1 wave re-anchors through express signals; a group link has only the engine's own follow-the-packet rule. Both ends send from the start, so a path either end can reach carries.
- **No relay media**, as for 1:1 waves.
- **A mesh, not a relay.** Up to seven others fit the slots; past a handful of people the uploads multiply, and the any-size doctrine wants a member fanning out instead.
