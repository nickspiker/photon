---
name: per-device-lanes
description: "SHIPPED 2026-08-13 photon 53ad8f9 (unpublished): every device writes its own lane; convergence = lane-wise CRDT merge; canonical row order (eagle_time, blake3(content)) incl. at-rest keys; fleet-forward demoted to dead-origin backstop"
metadata: 
  node_type: memory
  type: project
  originSessionId: 81588914-5914-4600-bb98-72cc4fae2260
  modified: 2026-08-14T00:19:44.783Z
---

Nick approved dissolving the single-writer send path (2026-08-13, "per device lanes... your call on how"). SHIPPED photon 53ad8f9, tests pass, NOT yet published to the fleet.

What was already built (the lane layer was ~complete): mint_our_lane on first send, ensure_lane receive-anywhere (friend materializes any lane from root ‖ wire label), merge_lanes_from = the lane-wise CRDT (union of lanes, per-lane strictly-greater fast-forward, device-local our_label/pendings/send-tip never adopted — sanitize_replicated), era supersede for re-keys, lane rotation. drain_chain_syncs already used the lane-wise merge.

What 53ad8f9 changed — the three gates that forced every non-owner device to fleet-forward through the chain owner:
1. chain_transmit demanded LOCAL CLUTCH-Complete (§4.2 parks non-owners at Pending forever) → now Complete OR lane_capable (replicated chains with live lane_root; the root only exists post-ceremony, so holding it proves the friendship completed somewhere in the fleet). FriendshipChains::lane_capable() added.
2. drain_chain_syncs never wired contact.friendship_id on adopt (chains sat unreachable in RAM; boot only loads contact-referenced fids) → wired + persisted at the adopt, log "CHAIN-SYNC: wired ... transmits on its own lane now".
3. compose_ready + the fleet-forward drain keyed on per-device chain_woven → both take lane_transmit_capable(ci) too. The drain stays as the DEAD-ORIGIN backstop (rows replicated, origin died before delivery → a sibling re-serves on ITS lane; friend dedups the both-alive overlap).

CONVERGENCE (Nick's question answered): merge is a join-semilattice — all copies converge to the union of lanes at max positions, order-free. Test `per_device_lanes_converge_on_cross_merge` pins it.

SAME-TICK ORDERING (Nick's frank question): canonical total order everywhere = (eagle_time, blake3(content)). Was: RAM insert timestamp-only (ties in ARRIVAL order → divergent render + divergent order-dependent anti-entropy digest → endless history walks between converged devices) AND the vault row key was BARE eagle_time — same-tick rows from two senders shared a primary key, second sender's message SILENTLY OVERWRITTEN at rest. Now: row key = BE(eagle_time) ‖ blake3(content)[..8] (byte order == canonical order), legacy Int keys read-compatible + swept on next save (self-terminating). Weave resolve is per-sender-stream (outgoing-only, 704ps ticks) so it never needed the tiebreak.

Heathrow airport-wifi finding (same day): mixed direct/relay pairs = asymmetric multicast (Android hears beacons + discovers LAN peers, Mac only transmits; client unicast partially isolated — Mac→Android LAN undeliverable). Zero direct inbound pings at either Nick device → reflection/reflect-bootstrap correctly had no trigger. Not a code failure; that network.

Related: [[messaging-solidity-phase-a]] (this IS the §14 direction), [[lane-rotation-wedge-heal]] (the friend-side new-lane adoption the design reuses), [[self-pair-sibling-row]].
