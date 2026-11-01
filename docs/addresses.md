# Addresses: every one a device has

Nick 2026-11-01: "Any other one address per device issues too? I could have two ipv6 addy's per device, or four" … "new phonebook format with extensible/unlimited entries apparently. up to you on how you want to group those and tell if they are stale or not."

The field case: Emma's desktop and phone on one LAN with Nick's Mac and Brittany's phone. A LAN beacon's address landed on the PERSON (`Contact::local_ip`), overwritten by whichever of her devices beaconed last, so group links knew no LAN address for either device, aimed at the router's public address (which it does not hairpin), and her desktop carried not one packet in a group beam.

## Stage 1 — every address a device is heard at (built)

`DeviceEndpoint.addrs` replaces the one `public` and one `lan` slot: every address the device has been known at, newest first, each a `SeenAddr { addr, how, heard }` — how it was learned (`AddrSource::Heard` from its own traffic, `Beacon` from a LAN beacon naming the device, `Record` from a signed phonebook or gossip record) and when (Eagle time).
`note` moves an address to the front (a re-heard address is refreshed, never duplicated), `forget` drops one (another device of the contact holds that NAT mapping now), and at most `DEVICE_ADDRS_MAX` (16) are kept, the least recently heard going first.
`public()` / `lan()` are the freshest of each kind; `all()` is every one, LAN first; candidate gathering (`traverse::gather`) offers every address, so the stable priority sort tries the freshest first within each kind.
A LAN beacon carries its device key, and its address now goes to that device's set.

## Stage 2 — our own addresses, all of them

A device publishes every address it can be reached at, not the one `our_reflexive` slot (which held v4 OR v6):
- each interface address (getifaddrs on Unix and Android): global IPv6 addresses (stable and temporary), LAN IPv4, VPN interfaces;
- the reflexive public v4 and v6, kept apart (`ReflexiveState` already does in the receive loop; the app collapsed them);
- the Wi-Fi Direct group address while a group is up.

## Stage 3 — the phonebook record: a set

A new record kind beside the old one, `DeviceAddressSet` (kind 4) at `key_address_set(device)`:
- the 256-byte slot holds the device, the identity's handle proof, a pointer to the SET, the publish timestamp (the slot's epoch, at the same offset as `DeviceAddress`), and the egg pointer;
- the set is a content-addressed VSF document stored beside the eggs: one entry per address — the socket address, its kind (global v6, LAN v4, reflexive v4, reflexive v6, Wi-Fi Direct, VPN) and when this device last saw itself there;
- the device signs domain ‖ handle ‖ device ‖ set pointer ‖ timestamp, so the set is covered by the signature through its hash.

Staleness: every publish replaces the whole set — an address the device no longer has is simply not in the next one, and a reader takes a set only if it is newer than the one it holds (the epoch guard the worker already applies). Within a set, `seen` orders the entries, so a reader tries the freshest first; nothing ages on a timer.

Rollout: the worker learns the new kind first (it verifies with the same fgtw code); devices publish BOTH records while older builds live (an old build never sees kind 4 and keeps reading kind 3), and read the set first when it is there.
