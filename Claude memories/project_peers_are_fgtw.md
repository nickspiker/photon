---
name: project_peers_are_fgtw
description: "Decentralize FGTW — fgtw.org server retires, peers become the trust web; open phonebook + mutual-consent CLUTCH. Full design in docs/peers-are-fgtw.md"
metadata: 
  node_type: memory
  type: project
  originSessionId: a24c64ab-95e4-4edf-8dc3-366eec9b6268
---

Architecture direction set 2026-06-28 (design only, not implemented). Full doc: [docs/peers-are-fgtw.md](/mnt/Octopus/Code/photon/docs/peers-are-fgtw.md).

**The vision:** `fgtw.org` (central bootstrap SERVER) retires; FGTW the NETWORK stays — the peers themselves ARE the FGTW. Gossip mesh now, full Kademlia DHT ("kamadilla") eventually. Two openness levels, deliberately different:
1. **Phonebook is OPEN** — ask any peer "give me everyone who's ever attested", they hand it over. Ungated enumeration. ("I'll send you the phonebook.")
2. **Conversation/identity is MUTUAL-CONSENT** — someone can SEND a CLUTCH but if you haven't friended them back, you ignore it ("yeahnah"). Activates only when BOTH friend each other. CLUTCH completion IS the mutual handshake. Then avatars/identity unlock.

**Already built — do NOT rebuild (the consent half is done):** ping/pong is contact-gated (`status.rs:1329-1336` drops non-contact pings silently); CLUTCH receipt contact-gated everywhere; CLUTCH is inherently two-sided (auto-offer only to saved contacts at `app.rs:2915-2919`; completion needs both offers+proofs); handle string never on wire (only handle_proof); `PeerStore::get_all_peers()` exists in-memory (`peer_store.rs:69`) but is never called; `node.rs` has real Kademlia structures but they're DEAD scaffolding (never instantiated); `FgtwMessage::Pong{peers}` is an unused ready-made gossip carrier.

**Net-new work, ranked:**
- **Phase A (immediate ask): share the phonebook.** PeerStore is in-memory only, populated SOLELY from fgtw.org, rebuilt empty each launch. Need: persist it to the vault (e.g. vault_key("peers", vault_seed)); add a P2P enumeration request (repurpose Pong{peers} or new GetPeers/Peers); feed PeerStore::add_peer from peers (today only handle_query.rs writes it, from server); gossip/anti-entropy on connect. PeerRecord = {handle_proof, device_pubkey, ip, local_ip, last_seen}, per-device — adequate, no schema change.
- **Phase B:** move announce/search + wss live-push off fgtw.org to peer-sourced; keep fgtw.org bootstrap-only.
- **Phase C:** avatars are PUBLIC BY DESIGN (open GET by handle-derived key is intended, NOT a consent leak — nothing to gate; corrected from an earlier draft). The real point: avatars are big (~hundreds of KB, 256x256 AV1) and must stay OUT of the phonebook — the registry gossips tiny records only ({handle_proof, device_pubkey, ip, local_ip, last_seen}) so enumeration stays cheap. Avatar = separate public on-demand P2P pull by key (already separate today; keep it separate when FGTW goes P2P). Rides deferred device-sync bulk-content phase ([[project_vault_roadmap]]).
- **Phase D:** real Kademlia — wire node.rs routing table into iterative FIND_NODE; revive dead FgtwMessage::{FindNode,FoundNodes}.
- **Phase E (small/optional):** explicit "I/they/both initiated" consent state + friend-request UX; mechanism already mutual, only modeling missing. Extends `Contact::clutch_status_detail()`.

**Scale threshold (2026-07-03, user-stated):** flat everyone-mirrors-everything registry until **~100k peers** — no Kademlia routing/splitting ("kamadilla") before that, full stop ("Right now? yeahnah"). Registry updates (public card: peer record/IP, membership chain, avatar) ding-dong PHOTON-WIDE (everyone mirrors them); fleet-sealed state (roster/friendship/streams) dings fleet-only. Scopes are in the fleet-sync.md kinds table.

**Notification-order doctrine (2026-07-03, user-stated):** every node mirrors the ENTIRE phonebook, so state changes ding-dong peers FIRST — directly, at mirrored addresses (PeerRecord is self-signed, messenger needs no trust) — and FGTW is told LAST (durable straggler-catcher, never the primary channel). Canonical flow: my IP changed → sign record → P2P to fleet siblings + live friends → announce updates the slot for whoever slept. Today's FGTW-first bump flows are training wheels; the mirror inverts them with no payload change. Written into docs/fleet-sync.md §6 alongside the whole bump architecture (doorbell-not-package, three sauces, heads-not-versions diffs, one-CLUTCH-then-fleet-inherit).

fgtw.org hardcoded in 4 files: bootstrap.rs:6, blob.rs:5, relay.rs:9, avatar.rs:932 (+ seed pubkeys bootstrap.rs:16-25).

Relates to [[project_storage_layering]] (vault), [[project_clutch_offer_deadlock]] + [[project_chain_advance_desync]] (the CLUTCH/chain we debugged this session — CLUTCH is the consent handshake here).
