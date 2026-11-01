---
name: project_address_sets
description: 2026-11-01 docs/addresses.md — every address per device; stage 1 BUILT (DeviceEndpoint.addrs, beacon per device); stage 2 own-address enumeration + stage 3 phonebook DeviceAddressSet (kind 4, VSF set blob, worker first, dual publish) next
metadata:
  type: project
---
Nick 2026-11-01 asked for unlimited addresses per device ("two ipv6 addy's per device, or four") and a new extensible phonebook format, staleness up to me.
Stage 1 BUILT (657681e4): DeviceEndpoint.addrs = Vec<SeenAddr{addr, how: AddrSource, heard}>, newest first, cap 16; public()/lan()/all()/has()/note()/forget(); gather offers every address; a LAN beacon's address goes to the device it names (was overwritten on the person's local_ip).
Stage 2 (next): enumerate own interface addresses (getifaddrs), keep reflexive v4 and v6 apart (app's our_reflexive collapses them), WFD.
Stage 3 (next): fgtw RecordKind::DeviceAddressSet=4 at key_address_set(device): slot = device/handle/set ptr/timestamp@99/eggs ptr; set = content-addressed VSF doc of {addr, kind, seen}; signature over domain‖handle‖device‖set ptr‖timestamp. Worker (/mnt/Harbor/Code/fgtw-bootstrap, path dep on fgtw) learns kind 4 FIRST; devices publish both kinds; staleness = whole-set replace + epoch guard + per-entry seen ordering.

**Why:** Emma's desktop carried zero packets in a group beam — no per-device LAN address, public address not hairpinned.
**How to apply:** old readers drop kind 4 as Empty (safe); a changed kind-3 signature would break every old reader — never touch kind 3's signed bytes.
