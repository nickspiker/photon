---
name: project-chain-replication
description: fleet CHAIN replication SHIPPED 2026-07-25 @ 2dfe7ed — chains sync fleet-wide, behind-adopts (mutated_osc v7), any device becomes sendable on adopt; E2E pending
metadata:
  type: project
---

Fleet chain replication SHIPPED @ photon 2dfe7ed (user's model: "if another device is ahead I just catch up"):
- FriendshipChains.mutated_osc (schema v7, stamped by every mutator) = the ordering key; adopt iff STRICTLY newer (no regression, no echo — adopted stamp recorded as pushed).
- chains_to_vsf_bytes/chains_from_vsf_bytes = canonical bytes round-trip (decoder reads fid from bytes); chain_sync frame = those bytes kete-sealed under the FLEET key, device-signed; recv worker packet-acks like hist_page.
- drive_chain_replication per tick pushes newer-than-last-pushed FRIEND chains to all siblings (sibling 1:1 chains never replicate); adopt arm flips the contact Complete+chain_woven → the device becomes directly SENDABLE.
- Supersedes the custody question; the fleet-forward path (project_fleet_unification_v1) remains the fallback for a device with no chain copy yet.
- Known accepted risk: concurrent same-instant sends from two devices fork the braid (§14 linearizer = the real serializer); catch-up shrinks the window to transport latency, fork-repair (reset + re-key streak 3) backstops.
- E2E PENDING on dev v0.48.8 (mac) / v0.48.9 (android): send from a non-owner device, watch CHAIN-SYNC adopt + direct transmit.
