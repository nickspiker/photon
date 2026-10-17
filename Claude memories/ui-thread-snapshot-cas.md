---
name: ui-thread-snapshot-cas
description: "SHIPPED 2026-08-08: nothing but UI on the UI thread — workers get snapshots, commits CAS against live state, writers fire ACK/transmit only after the durable write"
metadata:
  type: project
---

Shipped 2026-08-08 (f548560, f2eede5, fdee1fc); the laws every future offload follows:
- **Snapshot + CAS, not owner-thread:** the UI owns friendship_chains; workers get clones; drains re-gate against CURRENT state and CAS before committing. A voided commit mutates nothing — retransmit / held-row sweeps re-enter cleanly.
- **Durable-then-signal:** the coalescing chains writer fires gated signals (receive's ACK, send's transmit) only after the write lands.
- **Garbage is fork evidence ONLY when the CAS passed** — a decrypt on moved lane state never feeds the fork detector.
- **Gap buffer = free serialization:** frame N+1 fails chain-link verify until N commits, then re-enters the full gates.

2026-08-15 (c48b0e1): CLUTCH KEM decap moved too (4th job stage with CAS) and the ceremony chains save rides the writer. Still deliberately inline: startup loads, the 32-byte fanout pair store, save_contact.
Related: [[feedback_render_never_touches_vault]], [[edges-not-timers]].
