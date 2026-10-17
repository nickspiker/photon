---
name: project_peers_are_fgtw
description: "Decentralize FGTW: fgtw.org shrinks to the phonebook, peers become the trust web; open phonebook + mutual-consent friendship (SHIPPED 2026-08-25); docs/peers-are-fgtw.md"
metadata:
  type: project
---

Canonical: docs/peers-are-fgtw.md (direction set 2026-06-28).
**Vision:** fgtw.org (the server) retires toward bootstrap/phonebook only; FGTW the NETWORK stays — the peers ARE the FGTW. Two openness levels:
1. **The phonebook is OPEN** — any peer hands over everyone who has attested; records stay tiny ({handle_proof, device_pubkey, ip, local_ip, last_seen}); avatars are public by design but stay OUT of the phonebook (a separate on-demand pull).
2. **Friendship is MUTUAL-CONSENT** — SHIPPED 2026-08-25 @ f33ebec: adding is local-only (`Contact.consent_mutual = false`, no keygen); a tiny signed `friend_knock` (the conversation token is the proof) travels instead of the offer; a stranger's knock is silently dropped and remembered nowhere; a matched knock or offer flips Mutual → keygen → ceremony.
**Scale (2026-07-03):** a flat everyone-mirrors-everything registry until ~100k peers — no Kademlia before that ("Right now? yeahnah").
**Notification order (2026-07-03):** state changes go to peers FIRST (siblings + live friends, at mirrored addresses), FGTW LAST as the straggler-catcher (docs/fleet-sync.md §6).
**Next phases:** persist + share the phonebook peer to peer; move announce/search + live push off fgtw.org; real Kademlia (node.rs scaffolding is dead today) only past the scale threshold.
2026-10-17: Nick restated FGTW as "the address book" — the bind request moves device to device ([[project_bindreq_peer_to_peer]]).

**Why:** a central server is a dependency, a failure point and a trust surface; the commons should run on its peers.
**How to apply:** anything that would store state on fgtw.org beyond where-a-device-is needs a reason; peer-first, server-last.
