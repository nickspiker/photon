---
name: project-chain-replication
description: fleet CHAIN replication SHIPPED 2026-07-25 @ 2dfe7ed — chains sync fleet-wide, adopt iff strictly newer (mutated_osc), any device becomes sendable on adopt
metadata:
  type: project
---

Nick's model: "if another device is ahead I just catch up."
- `FriendshipChains.mutated_osc` (stamped by every mutator) is the ordering key; adopt iff STRICTLY newer (no regression, no echo).
- `chains_to_vsf_bytes` / `chains_from_vsf_bytes` round-trip canonically; the chain_sync frame = those bytes sealed under the FLEET key (now the epoch seal, [[project_fleet_epoch_arc_closed]]), device-signed.
- `drive_chain_replication` per tick pushes newer FRIEND chains to all siblings (sibling 1:1 chains never replicate); adopting flips the contact Complete + chain_woven, so the device can transmit directly. Fleet-forward ([[per-device-lanes]]) stays the fallback for a device with no copy yet.
- Accepted risk: concurrent same-instant sends from two devices fork the braid; per-device lanes ([[per-device-lanes]]) are the serializer now.
