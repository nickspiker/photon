---
name: project_fleet_routing_scale
description: "fleet invariants — any fleet size (design for 12+, never 2-device shortcuts); reply-TX targets the friend device you last heard from (Contact.active_device), the rest of their fleet still gets everything as fast as routable"
metadata:
  type: project
---

Two standing design rules from Nick (2026-07-03):
1. **Any fleet size.** A fleet must work at N=12, not just the 2-device rig — egg lists, fan-out re-key, braid-in of a fresh device, roster/fstate sharing. Never write "the other fleet device" logic; always "each sibling device."
2. **Reply routing.** The device a friend's message came from is presumed in their hand and is the primary TX target; the REST of their fleet still gets every message, best-effort, never gated on the active device.

**Built:** per-device addressing (Nick approved 2026-07-15 after the single `contact.ip` slot flip-flopped between a friend's devices, cancelling 548 KB offers mid-flight): `Contact.device_endpoints: Vec<DeviceEndpoint>` + `Contact.active_device` (types/contact.rs); relay forwards address every FOLDED member, not just devices with a cached endpoint (fleet-sync.md §7).
**How to apply:** traffic learned from a device goes back to THAT device's address; fan-outs walk the fold. Related: [[project_keyring_design]], [[project_rarangi_messages_fleet]].
