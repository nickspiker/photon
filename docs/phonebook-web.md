# The phonebook web

Status: **design, 2026-11-10.** Phase 0 (below) is built and deployed; phases 1 and 2 are not.
Supersedes the open phases of `peers-are-fgtw.md` (B, C, D); that doc's Phase A and A′ stay as written.

## The rule

Nick 2026-11-10: the bootstrap (fgtw.org) is contacted ONLY to attest a brand-new device, or when every peer is offline, or when every peer's address has changed and nobody answers.
Everything else is FGTW: the Fractal Gradient Trust Web.
Trust runs outward from you: your own devices, then your contacts, then theirs; a lookup weighs answers by how close the answerer is.

## What every device serves

By default every device serves a share of the phonebook: everything fgtw.org hosts today except the sealed fleet settings blobs (fstate, space), which replicate among your contacts separately, sealed under your key.
A node never stores anything readable for strangers: public signed records, or sealed ciphertext only.

| Served on the web | Kind |
|---|---|
| Phonebook records (`fgtw::phonebook`: address, address set, pointer, identity count) and their egg / set blobs | public, signed |
| Membership chains, by handle proof | public, signed, self-verifying |
| Succession records | public, signed |
| Fan-out blobs | sealed inside a signed document |
| Bind requests (short-lived) | public, signed |
| Avatars (pin-encrypted) and the contacts backup blobs | sealed |

Stays on the bootstrap: the relay pipe, the hub, doorbells (they hold push-service credentials), the bind-refusal inbox, gripes and diagnostic logs.
The worker-written indexes (`device_owner`, `plate_owner`, `device_lock`) stay there too until they become signed records (Phase 3).

## Serving roles, automatic

No off switch; the share follows what the device can afford.

| Device state | Serves |
|---|---|
| Always on, unmetered, on power (desktops) | a full share and extra replicas |
| Phone on Wi-Fi and charging | a normal share |
| Cellular, or battery saver | its own fleet and its contacts only |

Device-local `serve.*` caps (storage, bandwidth) follow the beam settings pattern, in an advanced corner.
Charging and power-saver signals are new on Android (BatteryManager / PowerManager to JNI, beside `nativeOnMetered`).

## The web

- **Node id**: BLAKE3 of a domain tag and the device key, in the same keyspace as the `fgtw::phonebook` record keys.
- **Routing**: `network/fgtw/node.rs` (XOR distance, k-buckets) revived, k = 1<<3. Filled from siblings, contacts, every validated peer, and, for a brand-new device only, the bootstrap.
- **RPCs**: `find_node`, `find_value`, `store`, as signed frames on the existing ladder (fgtw::protocol `signed_frame`, a StatusUpdate arm, `STRANGER_SECTIONS`). Strangers may ask; a stranger whose address has not round-tripped gets at most k contacts or one small record, so the web cannot be used as an amplifier. PT carries large values.
- **Storage**: one vault key per node store, capped by the role; adoption reuses the existing rules (newest-wins records, extension-only chains with the genesis pin, the phonebook epoch guard).
- **Replication**: the owner stores to the k closest on its own change edge and when a closer node appears; holders re-replicate on the same routing edge; records expire by their own epoch.
- **Lookup with the trust gradient**: ask your own fleet, then your contacts (they answer from chains they have pinned), then the k closest.

## Changes travel to the people who care

Nick 2026-11-10: "if I do any change to the phonebook, avatar, device removal, whatever, it should get broadcast to all my friends that are online, then to the bootstrap (FGTW first, fgtw.org second) because ideally they update their friends ... and it's timestamped so we know what's fresh."

- **One edge.** A signed change goes out the moment it exists: to the person's own devices and to every friend online, then it is written to fgtw.org, which keeps a copy and is read only on the last rung of the ladder.
- **Passed on, once.** A device that adopts a *newer* record (a newer signed stamp, or for a chain an extension) passes it to its own devices and to the record owner's devices it can reach; something it already had stops there, so nothing loops and nothing floods.
- **Freshness is in the record.** Phonebook records carry their signed epoch, chains grow only by extension (the held head is the stamp), the avatar pointer and settings carry their own stamps; a receiver never needs fgtw.org to know which copy is newer.
- **No polling.** Nothing asks on a timer what it can be told on a change.

| Change | Status |
|---|---|
| Chains (device added, removed, locked, declared) | **Built 2026-11-10** (`chain_news`): a new held head goes to our siblings and contacts (our chain) or to our siblings and the owner's devices (a contact's chain). Our own writes still reach fgtw.org first inside the fgtw client, a moment before the mesh push. |
| Addresses | **Built 2026-11-10**: pushed to contacts on change; a friend's fresh own push is passed on to our siblings, once (`pass_address_on`), and a record a sibling passes on goes no further. |
| Avatars and names | **Built 2026-11-10**: a changed avatar pin or name owes every contact one announce pong, sent with a ping round at once (`announce_profile`); the pong salvage carries the sealed name and pin. |
| Settings and roster | **Built 2026-11-10**: pushed to siblings first; the fgtw.org backup merges into the slot as last seen and writes by compare-and-set (`merge_into_slot`, the worker's `prev`), reading fgtw.org only when it answers that the slot moved. |
| Join requests | **Built 2026-11-10**: local first (LAN beacon, the direct bind_req, NFC). The joining device posts to fgtw.org, listens to its hub and reads its chain copy only after "My devices aren't nearby: ask through fgtw.org"; the adding device reads fgtw.org's requests only after "Not nearby? Look through fgtw.org". |

## First claim on a handle

The one question without a server: who claimed a handle first.

- A chain any of your contacts has pinned wins: they have talked to that person.
- With no contact holding it, the chain is accepted when the k closest holders agree.
- A genesis conflict is shown to the person adding the contact, never resolved silently; while the bootstrap exists it breaks the tie.
- Forging a first claim needs the handle itself (the genesis is co-signed by a key derived from it), so the attacker is someone who already knows it.

## Phases

0. **Built 2026-11-10.**
   - Octopus actions need the admin sign-in, and its capsule goes to signed-in sockets only.
   - Held chains pin their genesis.
   - `ensure_member` never mints over a held chain.
   - `blob_put` gates on fleet membership; avatar slots are owned by their identity; bells come only from the identity's own devices.
1. **The rule with what exists**: no bootstrap reads on an existing device's start, settings change or address change; the online signal from peers; own chain from siblings, with an explicit unanswered step before the bootstrap.
2. **The web**: node id and routing, RPCs, node store, replication, gradient lookup, roles; then `PeerStore` gossip folds into it and the relay pipe open is signed by the device.
3. **Signed indexes**: device ownership, plate ownership and device locks as signed records, so they can leave the bootstrap.
