---
name: re-clutch-never-store
description: Recovery doctrine — re-run the CLUTCH ceremony rather than persist/back up secrets; secrets at rest only when absolutely required
metadata: 
  node_type: memory
  type: project
  originSessionId: faa32b1c-4aed-43dc-84b7-06eb9c63c556
  modified: 2026-08-02T10:00:41.906Z
---

Decision (Nick, 2026-08-02): **re-clutch always; never store secrets if not absolutely needed.** The parked idea of backing up CLUTCH pair secrets in the cloud blob (so a wiped device recovers friend content without re-clutching) is REJECTED — it widened what a compromised fleet key exposes.

**Why:** a ceremony is repeatable; a leaked secret is forever. Recovery cost (one re-clutch per pair) is acceptable by design.

**How to apply:** when a wipe/upgrade/migration leaves a pair without keys, the answer is a fresh ceremony, not a persisted copy. Reject designs that stash pair secrets, ratchet state, or derived keys anywhere beyond the device registers/vault that already must hold them. Related: [[edges-not-timers]].
