# Addresses: every one a device has

Nick 2026-11-01: "Any other one address per device issues too? I could have two ipv6 addy's per device, or four" … "new phonebook format with extensible/unlimited entries apparently. up to you on how you want to group those and tell if they are stale or not."

The field case: Emma's desktop and phone on one LAN with Nick's Mac and Brittany's phone. A LAN beacon's address landed on the PERSON (`Contact::local_ip`), overwritten by whichever of her devices beaconed last, so group links knew no LAN address for either device, aimed at the router's public address (which it does not hairpin), and her desktop carried not one packet in a group beam.

## Stage 1 — every address a device is heard at (built)

`DeviceEndpoint.addrs` replaces the one `public` and one `lan` slot: every address the device has been known at, newest first, each a `SeenAddr { addr, how, heard }` — how it was learned (`AddrSource::Heard` from its own traffic, `Beacon` from a LAN beacon naming the device, `Record` from a signed phonebook or gossip record) and when (Eagle time).
`note` moves an address to the front (a re-heard address is refreshed, never duplicated), `forget` drops one (another device of the contact holds that NAT mapping now), and at most `DEVICE_ADDRS_MAX` (16) are kept, the least recently heard going first.
`public()` / `lan()` are the freshest of each kind; `all()` is every one, LAN first; candidate gathering (`traverse::gather`) offers every address, so the stable priority sort tries the freshest first within each kind.
A LAN beacon carries its device key, and its address now goes to that device's set.

## Stage 2 — our own addresses, all of them (built)

A device publishes every address it can be reached at (`own_address_set` in `ui/photon_app/peers.rs`), most useful first:
- the reflexive public address peers saw (`our_reflexive`);
- the beacon-proven LAN address;
- each up, non-loopback interface's own addresses at our listening port (`udp::own_interface_addrs`, getifaddrs on Unix and Android): usable LAN v4 (never the CLAT `192.0.0.4`), global and unique-local v6 — stable and temporary alike; link-local v6 is left out (it needs a scope id no peer knows);
- an interface named `p2p…` is tagged Wi-Fi Direct.

Windows enumerates thru GetAdaptersAddresses (2026-11-01; Wi-Fi Direct not told apart there). Watch: Photon's main socket binds `[::]` and Windows defaults a v6 socket to v6-only, so a Windows device may not receive v4 on it at all — untested, flagged to Nick. The app keeps the newest public address in `our_reflexive` and the other family's in `our_reflexive_other` (2026-11-01), and the set publishes both.
The set is republished on the same edge as the single record (the reflexive address moving, or Android's interface-change edge); a v6 temporary address that rotates while the reflexive holds still waits for the next such edge.

## Stage 3 — the phonebook record: a set (built; worker deployed 2026-11-01)

A new record kind beside the old one, `DeviceAddressSet` (kind 4) at `key_address_set(device)`:
- the 256-byte slot holds the device, the identity's handle proof, a pointer to the SET, the publish timestamp (the slot's epoch, at the same offset as `DeviceAddress`), and the egg pointer;
- the set is a content-addressed VSF document stored beside the eggs: one entry per address — the socket address, its kind (global v6, LAN v4, reflexive v4, reflexive v6, Wi-Fi Direct, VPN) and when this device last saw itself there;
- the device signs domain ‖ handle ‖ device ‖ set pointer ‖ timestamp, so the set is covered by the signature through its hash.

Staleness: every publish replaces the whole set — an address the device no longer has is simply not in the next one, and a reader takes a set only if it is newer than the one it holds (the epoch guard the worker already applies). Within a set, `seen` orders the entries, so a reader tries the freshest first; nothing ages on a timer.

Rollout: the worker learns the new kind first (it verifies with the same fgtw code); devices publish BOTH records while older builds live (an old build never sees kind 4 and keeps reading kind 3), and read the set first when it is there.

As built: `fgtw::phonebook` `RecordKind::DeviceAddressSet`, `sign_device_address_set`, `verify_address_set_with`, `address_set_blob` / `address_set_entries` (one `addr` section per address: `ip` as `ni`/`nj`, `port` as `np`, `kind`, `seen` as Eagle time), at most `ADDRESS_SET_MAX` (32) entries and `ADDRESS_SET_BLOB_MAX` (8 KiB).
The worker stores the set at `pb/set/{ptr}` and serves it beside the record in `pb_get` and `pb_devices`.
Photon publishes both records on every own-address publish; `fetch_devices` returns each pointed device's verified set, and the resolver notes every address in it on that device's endpoint (`adopt_endpoint_into`'s `more`).
