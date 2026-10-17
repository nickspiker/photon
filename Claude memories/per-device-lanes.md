---
name: per-device-lanes
description: "SHIPPED 2026-08-13 (53ad8f9): every device writes its own lane; convergence = lane-wise CRDT merge; canonical row order (eagle_time, blake3(content)) incl. at-rest keys; fleet-forward = dead-origin backstop"
metadata:
  type: project
---

Nick approved dissolving the single-writer send path (2026-08-13, "per device lanes... your call on how"); shipped photon 53ad8f9.

Invariants:
- Any device transmits on its own lane once the friendship is `lane_capable()` (replicated chains with a live lane_root — the root only exists post-ceremony) — not only the local CLUTCH owner. `lane_transmit_capable(ci)` gates compose + the fleet-forward drain.
- `merge_lanes_from` is a join-semilattice: union of lanes at max positions, order-free; device-local our_label / pendings / send tip are never adopted. Test `per_device_lanes_converge_on_cross_merge`.
- Fleet-forward survives only as the DEAD-ORIGIN backstop (origin died before delivery → a sibling re-serves on ITS lane; friend dedups).
- Canonical total order everywhere = (eagle_time, blake3(content)); vault row key = BE(eagle_time) ‖ blake3(content)[..8] so byte order == canonical order. (Bare eagle_time keys let a same-tick row from a second sender silently overwrite the first at rest.)

**How to apply:** never key or sort rows on eagle_time alone; never route a send thru the chain owner when the lane is capable. Design of record: docs/lanes.md. Related: [[messaging-solidity-phase-a]], [[lane-rotation-wedge-heal]].
