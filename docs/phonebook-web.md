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
