# Dead-device dormancy

## The problem

A fleet device that is physically gone forever (the ocean phone, 1be949c1) can never sign its own departure, and the chain is consent-only — membership is permanent testimony, by doctrine, no exceptions.
Today permanence at the CHAIN layer leaks into the ROUTING layer: every device fans every row, ping, history push, and ceremony leg at the dead member forever.
Field cost, from one day of logs: hundreds of `RELAY: DROPPED — no mailbox` frames per device, the dead id participating in fleet rotation ("rotating to fleet member 1be949c1"), and chain-owning-sibling logic considering a device that will never answer.

## The verdict, not a timer

Dormancy is an EVIDENCE verdict, not an age: a device is DORMANT when every path has positively refused for a sustained span — relay says no-mailbox (pipe closed, nothing to hold), no LAN beacon, no pong, no signed frame of any kind since the span began.
The span is counted in fan-out attempts, not wall clock: N consecutive all-path refusals (N large — think hundreds, a weekend of storms) with zero authenticated inbound between them.
One authenticated frame from the device — a pong, a beacon, any signed row — voids the verdict instantly and restores full routing; waking is an edge, not a poll.

## What dormancy changes (routing layer only)

- Fan-out: chat/history/ceremony pushes SKIP the dormant device's legs; no relay frame is built for it.
- Rotation: a dormant device is not eligible as "active device" or a rotation target.
- Doorbell: never rung for a dormant device (the FCM token is dead anyway).
- Presence: shows as "dormant" in the fleet page — distinct from offline, honest about what we know.
- Probes: ONE cheap ping per periodic backstop sweep (~5 min) keeps the wake edge alive at negligible cost; this is the only traffic the device costs.

## What dormancy does NOT change

- Chain membership: untouched, forever — the verdict lives beside the roster, never in the chain.
- Keys and history: nothing re-keys, nothing is shredded; a dormant device that wakes decrypts everything it was always entitled to.
- Consent model: no one signed anything on the dead device's behalf; dormancy is each live device's OWN routing judgement, independently reached and locally held.
- Lockout: orthogonal — lockout is a security act against a device presumed hostile; dormancy is an economy act against a device presumed gone. A device can be both.

## Sync of the verdict

Each device reaches the verdict independently from its own refusal counts — no gossip needed for correctness, so no new consensus surface.
The fleet page MAY show sibling verdicts (via existing roster CRDT) purely as UI ("3 of 3 devices consider it dormant"), but routing always follows the local verdict.

## Status

Spec only — not built. Counting hook: the relay no-mailbox verdict and the ping TIMEOUT path both already flow thru status.rs; the counter and skip-gates land there when built.
