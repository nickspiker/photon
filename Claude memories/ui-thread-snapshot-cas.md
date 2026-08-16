---
name: ui-thread-snapshot-cas
description: "SHIPPED 2026-08-08: nothing but UI on the UI thread — workers get snapshots, commits CAS against live state, writers fire ACK/transmit only after the durable write"
metadata: 
  node_type: memory
  type: project
  originSessionId: 296bef77-7c97-45b8-8d90-dc492f93e557
  modified: 2026-08-08T14:56:22.372Z
---

Shipped 2026-08-08 (f548560, f2eede5, fdee1fc): every non-UI workload moved off the render thread. The design laws, which future offloads must follow:

- **Snapshot + CAS, not owner-thread**: the UI keeps ownership of friendship_chains; workers get clones; drains re-gate against CURRENT state and CAS before committing (receive: expected-prev still verifies; send: lane key unchanged). A voided commit mutates nothing — the retransmit ladder / held-row sweep re-enters cleanly.
- **Durable-then-signal**: the coalescing chains writer carries gated signals (receive's ACK, send's transmit) and fires them only after the write lands. Coalescing merges a superseded snapshot's signals onto its replacement; enqueue order = age order because the writer treats arrival as recency.
- **Garbage is fork evidence ONLY when the CAS passed** — a decrypt that landed on moved lane state must never feed the fork detector.
- **Gap-buffer = free serialization**: until frame N commits, N+1 fails chain-link verify and buffers; refills re-enter the arm's full gates via chat_replay_queue.

Deliberately still inline: CLUTCH KEM decap (once per ceremony, entangled with the ceremony state machine — move only when that code is next open), once-per-ceremony chains saves, startup loads.

Related: [[messaging-solidity-phase-a]], [[edges-not-timers]].

**Residue update 2026-08-15 (photon c48b0e1):** CLUTCH KEM decap and the once-per-ceremony chains save are MOVED (decap = 4th job stage with HQC-prefix CAS; chains save rides the writer with the proof as a gated ChainsPostDurable::CeremonyProof). Still deliberately inline: startup loads, the 32-byte fanout pair store, save_contact.
