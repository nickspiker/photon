---
name: project_fleet_routing_scale
description: "fleet invariants — any fleet size (design for 12+, never 2-device shortcuts) in eggs/braid/fan-out; reply-TX targets the friend device you last received from, rest of their fleet gets delivery as fast as routable"
metadata: 
  node_type: memory
  type: project
  originSessionId: 0b164fd9-062c-407e-b4bb-8f6be8d1982d
---

Two standing design rules from the user (2026-07-03):

**1. Any fleet size.** A fleet must work at N=12 devices, not just the 2-device test rig. Applies to the egg-lists (per-op signature lists, PQ-additive), the fan-out re-key on device-ADD (O(N) per-member slots is fine; nothing hardcoded to "the other device"), braid-in of a fresh device, and roster/fstate sharing. Never write "the other fleet device" logic — always "each sibling device."

**2. Reply routing heuristic.** When a message arrives from a friend, the specific DEVICE it came from is presumed to be the one in their hand — it is the primary TX target for replies (lowest latency to the active device). The REST of their fleet still gets every message delivered, as fast as routable — best-effort fan-out, not gated on the active device.

**Where the code stands (2026-07-03):**
- `Contact` stores ONE `local_ip`/`local_port` — last-beacon-wins. Wrong shape for N>2: needs a per-device address table keyed by `device_pubkey`. The pt_disc beacon carries the sender's device pubkey (`ke`) as of photon e0bb39f, so the keying data is on the wire already.
- fgtw `fanout/` slot is per-member within one identity slot (BRAID v0.2 §14.2) — already N-shaped.
- Braid is pairwise per-friendship; braid-in of added devices (the open piece in [[project_keyring_design]]) must be designed per-sibling, not "re-CLUTCH with the one other device."
- "Presumed active device" state (which peer device we last RX'd from, per friendship) is not tracked yet — needed for rule 2.

**CONFIRMED IN THE FIELD (2026-07-15, Robert/Nick logs):** the single `contact.ip` public slot is overwritten by pongs from ANY device of the contact's fleet (`knows_device` match), so with Nick on 3 devices the slot flip-flops between their addresses every cycle. Two user-visible failures, one root cause: (1) presence — pings only reach the last-ponging device, the rest hit 3× timeout and show offline despite validated traversal paths; (2) CLUTCH stuck at "5/8 awaiting their KEMs" — the 548KB offer transfer is cancelled mid-flight on every flap ("address changed — cancelling stale offer transfer"), and offers arriving FROM the sibling device are dropped as "unknown conversation_token". Fix = the per-device address table this note already prescribes (public + LAN per device_pubkey; ping/reply/ceremony traffic targets the address learned from THAT device). User approved building it 2026-07-15 ("per device addressing, yes please").

Related: [[project_keyring_design]], [[project_rarangi_messages_fleet]] (fleet = a conversation).
