# Peers Are FGTW — decentralizing discovery (design)

Status: **design only, not implemented.** Captures the target architecture so we build it in deliberate pieces.
Written 2026-06-28.

## The vision

`fgtw.org` (the central bootstrap/seed **server**) retires.
**FGTW the *network* stays** — the peers themselves *are* the FGTW (Fractal Gradient Trust Web).
The server is temporary scaffolding for bootstrap; the gossip mesh of peers becomes the registry + trust web, and eventually a full Kademlia DHT ("kamadilla" = the end-state DHT split).

Two separable concerns, with deliberately different openness:

1. **The phonebook is OPEN.** Any node can ask any peer on its peer list "give me everyone who has ever attested," and the peer hands it over. Discovery/enumeration is **public, ungated** — "I'll send you the phonebook."
2. **Conversation + identity reveal is MUTUAL-CONSENT.** Someone can *send* you a CLUTCH offer, but if you haven't friended them back: "yeahnah." You ignore it. The relationship only activates when **both** sides have added each other (both initiate). Then — and only then — avatars/identity/conversation unlock. **CLUTCH completion IS the mutual handshake.**

## What ALREADY works (do NOT rebuild)

The consent half is essentially done — the existing bilateral-contact gating already implements "ignore until mutual":

- **Ping/pong is contact-gated.** A `StatusPing` from a non-contact is silently dropped (`status.rs:1329-1336`) — the pinger learns nothing, not even liveness.
- **CLUTCH receipt is contact-gated everywhere.** Offer/KEM/Complete handlers reject unknown senders (TCP `status.rs:913-919,926,952,978`; PT `:1119-1123,1134,1164,1196`; SPEC `:2141-2150`). A stranger literally cannot open a CLUTCH with you — exactly "they can send, I ignore."
- **CLUTCH is inherently two-sided.** An offer is auto-sent only when the peer is *already a saved contact* and comes online (`app.rs:2915-2919`). Completion requires **both** parties' offers + matching `eggs_proof` (`contact.rs:101`, `all_slots_complete`). One side cannot complete alone. So "both must initiate" is already true in effect.
- **Handle string never hits the wire** — only `handle_proof` (memory-hard PoW). Good for the open phonebook (no plaintext handles leak).
- `PeerStore::get_all_peers()` **already exists in-memory** (`peer_store.rs:69`) — but is never called.
- `node.rs` has real Kademlia data structures (RoutingTable, KBucket, NodeId XOR distance, find_closest, 256 buckets) — but they're **dead scaffolding**, never instantiated outside node.rs. A library to build on, not a working DHT.
- `FgtwMessage::Pong { peers: Vec<PeerRecord> }` (`protocol.rs:13-16`) exists as a ready-made gossip carrier — currently unused (live path uses `StatusPong`, no peers).

## What's NET-NEW (the actual work), ranked

### Phase A — Share the phonebook (the immediate ask)
The registry is in-memory only, populated **solely from fgtw.org**, rebuilt empty each launch.
To make it peer-shareable + enumerable:

1. **Persist the PeerStore to the vault.** Today `PeerStore::new()` starts empty every launch (`photon_app.rs:778`); records only come from the server. Persist it (kete entry, e.g. `vault_key("peers", vault_seed)`) so the phonebook survives restarts and accumulates.
2. **Add a peer-to-peer enumeration request.** A wire message "give me all attested peers" (paged for large registries). Repurpose the unused `FgtwMessage::Pong { peers }` or add a dedicated `GetPeers`/`Peers` pair. Respond with `get_all_peers()` (already exists). **Ungated** — the phonebook is public.
3. **Feed PeerStore from peers.** Currently only `handle_query.rs` writes the store (from server). Wire the P2P enumeration response into `PeerStore::add_peer`, with dedup by `(handle_proof, device_pubkey)` (already the store's key) and last-seen reconciliation.
4. **Gossip / anti-entropy.** On connect to any peer, exchange phonebooks (or deltas). This is what makes "ask anyone, get everyone" actually converge across the mesh.

PeerRecord today = `{handle_proof, device_pubkey, ip, local_ip, last_seen}` (`protocol.rs:104-112`), per-device, multiple devices per handle.
Adequate for the phonebook; no schema change needed for Phase A.

### Phase A′ — The membership chain over the mesh — ✅ BUILT 2026-10-26
The phonebook was never the only thing the seed held: a fleet's MEMBERSHIP CHAIN (which device keys are this handle's) lived on fgtw.org alone, and two flows depended on that copy — a friend learning of our new device fetched our chain from the seed on the pong tail's `ftip` tripwire, and a joiner could only be bound through the seed's bind-request registry.
Nick, 2026-10-26: "I'm not sure why we have to rely on this fgtw.org unless everyone on our list does not respond."
So the chain now travels like the address records do:
- **Held on the device.** `fleet.rs` keeps every chain that folds (ours persisted to the vault as `fleet_chain`, reloaded at attest); `fetch()` asks the seed and serves the held copy when the seed is down or behind.
  Forks (two members appending with no seed to serialise them) are ordered the same way on every holder: longest valid chain, then newest tip, then lower head hash.
- **Two sibling frames.** `fleet_req` asks a device for a handle's chain (answered only to a device some fold knows — the chain is public, a 10–50 KB answer to anyone is amplification bait); `fleet_resp` carries it, as the answer or as a sponsor's unasked PUSH to its siblings and to the device it just bound.
  The carrier earns no trust: the receiver folds, checks the slot and the genesis pin, and adopts only a newer tip — the same drain a seed copy goes through.
- **Peers first, seed last.** A contact's fold refresh asks its own devices first; the seed is asked only for a contact with no reachable device, or one whose earlier ask is still unanswered when a refresh fires again (an edge, no wall clock).
- **The join over the mesh** (docs/pairing-v2.md): the joiner builds its binding request once and sends it two ways with one stamp — to the seed's registry when it answers, and straight to every sponsor device whose beacon it hears on the LAN (`bind_req`).
  The sponsor's matcher screens it exactly like a registry row; the bind lands in the held chain and is pushed to the newcomer and the siblings; the grow is sealed locally and pushed likewise (`fanout_resp`), the seed mirrored when it answers.
  A seed that cannot be reached no longer fails an add.
Still on the seed: the attest's announce (the phonebook write — this phase's next step is the attest degrading to the held chain when the seed is down), the device→owner index and first-come genesis (no peer equivalent; they only ever bound users of the seed), relay/pipe, doorbell, inbox, avatars.

### Phase B — Reduce server dependence
fgtw.org is hardcoded in 4 files (`bootstrap.rs:6`, `blob.rs:5`, `relay.rs:9`, `avatar.rs:932`) + seed pubkeys (`bootstrap.rs:16-25`).
Operations to move off the server, once the phonebook gossips:
- **announce/search** → peer-sourced records (the phonebook replaces lookup-by-handle).
- **wss://fgtw.org/ws live peer-IP push** (`peer_updates.rs`) → subsumed by gossip (peers propagate IP changes).
- Keep fgtw.org as a **bootstrap-only** seed (first-contact when you know no peers yet), then go peer-sourced.

### Phase C — Avatars: PUBLIC, but OUT of the phonebook
Correction to an earlier draft: avatars are **public by design** — the open `GET .../avatar/{key}` (key = `base64url(BLAKE3(BLAKE3(handle)||"avatar"))`, `avatar.rs:1058-1063,1419`) is intended, not a consent leak.
There is **nothing to gate**; anyone who knows the handle can fetch the avatar, and that's fine.

The real concern is **registry shape/weight**: an avatar is hundreds of KB (256×256 AV1).
It must **never be bundled into the phonebook**.
The phonebook gossips tiny records only — `{handle_proof, device_pubkey, ip, local_ip, last_seen}` — so "enumerate every attested node" stays cheap.
Avatars are fetched **separately, on demand, by their public key**, only when you actually need to render one.
This is already the shape today (avatar is a separate fetch, not a PeerRecord field).
The design just needs to PRESERVE that separation when FGTW goes P2P: the phonebook gossips lightweight records; avatar fetch becomes a separate **public P2P pull by key** (peer-hosted, replacing the fgtw.org GET).
Bulk content (avatars/attachments) moving to a peer/vault-hosted home is the deferred device-sync phase ([[project_vault_roadmap]]).

### Phase D — Real Kademlia ("kamadilla")
The end-state DHT: wire `node.rs`'s routing table into an iterative FIND_NODE lookup + RPC handlers (revive `FgtwMessage::{FindNode,FoundNodes}`, currently dead).
Replaces full-phonebook gossip with O(log n) structured lookup at scale.
The phonebook gossip (Phase A) is the interim until this lands.

### Phase E — explicit consent UX (optional/small)
The mechanism is already mutual; only the *modeling* is missing — no explicit "I initiated / they initiated / both" field.
If we want to *show* "they want to talk, you haven't friended back" (a friend-request affordance), add a direction/initiator field.
The `Contact::clutch_status_detail()` added this session already surfaces handshake stage; this would extend it to one-sided pending.

## Sequencing
Phase A (persist + gossip the phonebook) is the immediate, self-contained ask and unblocks everything.
B/C/D/E follow once discovery is peer-sourced.
Consent (the hard-looking part) is already built — leverage it, don't rebuild it.
