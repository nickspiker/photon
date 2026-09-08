# The path ledger — one home for address trust

Status: SPEC (2026-09-07, Nick + field conviction from the MacBook↔Pixel presence flap). The three staging fixes below are SHIPPED; the ledger itself is not built.

## The doctrine (write it once, stop rediscovering it)

Photon has exactly three levels of address trust, and each has one correct source of truth:

1. **Acceptance — never consults the source.** Every payload authenticates by signature or AEAD; a frame that verifies is the peer no matter how it arrived (UDP, relay pipe, WFD, carrier pigeon). A frame that doesn't verify is dropped, never "wrong address". Arrival path influences nothing but the ring colour.
2. **Candidacy — "this address might reach device X."** Signed self-claims (registry, fstate address publications), LAN discovery, WFD open-house, a pong's arrival source. Only the device itself can sign a claim, so third parties cannot inject candidates — but claims go stale (a phone rarely knows its own NAT mapping), so candidates are hints for the challenge scheduler, never send targets on their own.
3. **Proof — "this address delivers to device X right now."** A fresh nonce we sent to that address, signed and returned. Challenge-response, time-bounded, and the ONLY thing that steers traffic.

The security calibration that makes this sound: an **off-path attacker can never fake level 3** (it never sees our nonces), and an **on-path attacker who replays from its own address gains nothing its position didn't already grant** (it could drop or delay the stream at will regardless). Address caution is therefore not authentication — it protects the steering wheel from a redirect that signatures cannot prevent, because signed bytes are replayable.

Corollary, stated for the record: the pigeon delivers messages with full rights; it just doesn't get to update the flight plan.

## Structure

Per-DEVICE, never per identity — addresses belong to hardware on networks; the human has no address.

- **One ledger per remote device pubkey.** A friend with three devices publishing WAN+LAN each = three ledgers of two candidates on every one of our devices (never pooled: your desktop's LAN proving out says nothing about your laptop's).
- **Candidates attach to the remote device**: `(addr, provenance: self-claim | discovery | pong-source | punch-echo, published_at)`. Stable across OUR network moves.
- **Proofs attach to the path pair** `(local interface, remote addr)`: `(proven_at, rtt, tier: lan | wan | wfd)`. Invalidated wholesale when the local interface changes — from the kitchen your friend's LAN addr proves, from cellular it doesn't, and a network hop makes every proof suspect while every candidate stays valid. (This is the QUIC-shaped distinction, and it is exactly the Pixel wifi-associated/cellular-default split: proofs minted on one interface were being judged on another.)
- **One challenge primitive.** Ping/pong (nonce = per-send provenance, aimed address recorded) and punch/ack collapse into one prove-this-pair operation with one freshness rule.
- **Consumers ask one question**: "best proven path to device X, else relay." PT sends, media TX aim, chain-sync, presence fan-out — all read the ledger; none keep private address state.
- **The identity layer never picks addresses — it picks devices.** Sending to a friend fans to the whole fold (fleet invariants); each target device's ledger answers independently. The contact-row ring is the aggregate (best tier among the identity's devices); per-device dots show the underlying truth. A device with six candidates and zero proofs is simply reached by relay: orange, flowing.

## Migration sketch — the five sites that collapse into it

| today | becomes |
|---|---|
| `Contact::validated_path` + `validated_path_lan` + first-wins/LAN-supplants arms + PATH_TTL sweep | the ledger's proof table + ranking policy (rank = tier, then proven_at; re-challenge in background; "supplant" is just a better-ranked proof existing) |
| punch tiers / TRAVERSE ack handling | the challenge primitive's second producer — a punch ack IS a pair proof |
| PT frozen-address + `retarget_peer` on fresh pong | PT holds no addresses; each (re)send asks the ledger (kills the frozen-address class at the root) |
| media engine's address-follows-auth `peer` variable | stays local for latency (per-packet ledger reads are silly) but seeds from the ledger and reports back: a forward-progress re-point is a proof by AEAD |
| presence `is_online` verdicts / ring + dot colours | liveness stays crypto-only (any-path, per the two-tier window); colours = ledger's best proven tier, `contact_conn_tier` becomes a ledger read |

Ranking policy note: first-wins dies. The ledger aims at the best CURRENTLY-PROVEN path and keeps challenging better-tier candidates in the background, so a rotated cellular v6 strands nothing (the proof lapses, the next-best proven path takes over, the rotated candidate re-proves when the device republishes it).

## Staging fixes (SHIPPED 2026-09-07, ahead of the ledger)

1. **Salvage liveness is stamp-bounded** (±10 min on the pong's own eagle stamp): a captured pong can no longer paint a device online days later. Matched pongs need no stamp — the nonce is the freshness.
2. **Adoption steers at the AIMED address, not the arrival source**: `PendingPing` records where the ping was aimed; a fresh nonce-match proves THAT address delivered (provenance is per-send, so the match identifies exactly one aim). The arrival source — which an on-path replayer chooses and asymmetric NATs rewrite — no longer steers PT retargeting or the app-layer adopter.
3. **Media re-point requires forward progress**: only a strictly-newer authenticated seq may re-aim call TX. The step ratchet already killed cross-step replays; this closes same-step replay redirect. An off-path attacker never holds a newer authentic packet.

Also in this family, already landed separately: the two-tier ping window (strike at 5s, nonce-matchable to 90s — presence follows the crypto wherever it arrives, address trust stays fresh-only).
