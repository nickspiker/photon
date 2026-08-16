---
name: project-vault-roadmap
description: Phasing for Photon vault work — what belongs in the current dual-ring v1 vs. what waits for device-sync phase
metadata: 
  node_type: memory
  type: project
  originSessionId: d48050a2-34aa-4416-9f29-d6875b3be60b
---

Dual-ring `photon.vsf` (ring 0 = XDG config, ring 1 = XDG data) shipped 2026-06-08. Open-with-repair, randomized dual-write, degraded flag exposed. Lives in [[feedback-source-map]]-tracked `src/storage/flat.rs`.

**Deferred to the device-sync phase (not current scope):**
- Ring health introspection tools / vault inspector beyond `vaultinfo`
- Mid-session ring resurrection (today: failed ring stays dropped until process restart)
- Garbage collection / compact pass (orphaned objects accumulate)
- Cross-device mesh sync

**Why:** all four need a richer ring/vault tooling layer that's most cheaply built once, alongside device sync — building them piecemeal now means rewriting later when the sync model lands.

**How to apply:** when the user asks for vault-side improvements, default to "yes if it unblocks current Photon flows; defer if it's ring tooling / GC / sync prep." The bulk-content use cases that motivate the heavier tooling — avatars, file attachments, saved audio/video calls — also wait for this phase since they need GC + sync to be useful at scale.

Short-term next steps still in scope:
- UI banner reading `FlatStorage::degraded()`
- User-settings entry in root_commit dict (incremental pair-list, see [[feedback-source-map]])
- Wiring attestation → FlatStorage write so the root_commit dict actually populates
