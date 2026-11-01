---
name: project_address_sets
description: 2026-11-01 docs/addresses.md — every address per device; stages 1-3 BUILT (DeviceEndpoint.addrs, own getifaddrs set, phonebook DeviceAddressSet kind 4, worker deployed); open: Windows enumeration, v4/v6 reflexive split, republish on v6 rotation
metadata:
  type: project
---
Nick 2026-11-01 asked for unlimited addresses per device ("two ipv6 addy's per device, or four") and a new extensible phonebook format, staleness up to me.
Stage 1 BUILT (657681e4): DeviceEndpoint.addrs = Vec<SeenAddr{addr, how: AddrSource, heard}>, newest first, cap 16; gather offers every address; a LAN beacon's address goes to the device it names.
Stage 2 BUILT: udp::own_interface_addrs (getifaddrs, unix only; Windows returns empty) + peers.rs own_address_set (reflexive, beacon LAN, interface v4 then v6 at our port, p2p* = WifiDirect, cap ADDRESS_SET_MAX).
Stage 3 BUILT: fgtw 3c545bf RecordKind::DeviceAddressSet=4 at key_address_set(device), set = VSF doc of `addr` sections {ip ni/nj, port np, kind, seen e}; worker fgtw-bootstrap e1aec51 stores pb/set/{ptr}, serves `set` in pb_get/pb_devices — DEPLOYED 2026-11-01 (worker-only). Photon publishes kind 3 AND kind 4 from publish_self_peer_record; fetch_devices returns Vec<DeviceSet>; adopt_endpoint_into takes `more: &[SocketAddr]`.
Open: Windows GetAdaptersAddresses; app's single our_reflexive (v4 OR v6); set republishes only on the reflexive-move / Android interface-change edge.

**Why:** Emma's desktop carried zero packets in a group beam — no per-device LAN address, public address not hairpinned.
**How to apply:** old readers drop kind 4 as Empty and parse pb_devices non-strictly (the extra `set` field is safe); never touch kind 3's signed bytes. Related: [[project_group_media_scale]].
