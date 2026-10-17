---
name: project_keyring_design
description: "Multi-device keyring = the signed fleet membership chain (fold() is the auth rule, genesis identity-bound, announce gated); TOKEN patent is the authority; device-add is now pairing v2"
metadata:
  type: project
---

Authority: the TOKEN patent (`/mnt/Harbor/Code/TOKEN/patent/patent.typ`); Photon's implementation notes are docs/keyring.md.
**Fleet membership chain (shipped 2026-06-30; the v0 Merkle-root keyring is deleted):**
- `src/network/fgtw/fleet.rs` `MembershipBlob` = ordered `FleetOp` chain {handle_proof, prev_hash, kind Genesis/Add/Remove, device_pubkey, eagle_time, signer_pubkey, sigs}. Sigs are an egg-list — every egg must verify. `fold()` IS the auth rule: valid sigs + hash link + signer was a member before this op; handle_proof is bound into every op (anti-transplant).
- Genesis is identity-signed (`identity_pubkey` + `identity_sig`; fold requires it on genesis and rejects it elsewhere); `genesis_identity_matches(seed)` lets a contact who knows the handle confirm a fleet is genuine. Genesis runs at attest, before the announce.
- The fgtw mirror verifies and only accepts forward-extending chains; the announce is gated on fold membership.
- **The device is a blind, stateless signing oracle** (fetch-then-sign; the network holds fleet state) — the load-bearing constraint. Count is visible to FGTW; count-hiding = a deferred modulus-style accumulator egg layered over the signed set.
- Patent model: one handle → MANY devices; one device → many handles allowed but owner-gated, off by default; the web invariant is anti-theft, NOT a cardinality cap. Lossy OWF = `ihi::spaghettify` ([[reference_ihi_primitives]]), not `ihi::handle_proof` (the memory-hard cost half).
- Device-add is pairing v2 ([[project_pairing_v2]]); removal is bilateral ([[self-only-removal]]).
**Residual:** FGTW can't cheaply verify ownership, so a squatter can first-come genesis with a fake identity_pubkey (peers who know the handle detect it); the fleet slot has no CAS.
Open as of 2026-07: braid-in of a freshly added device — likely closed since by chain replication ([[project_chain_replication]]); verify before working it.
**OPS TRAP:** device keys are fingerprint-deterministic, so a surviving `fleet/v1/` chain re-admits a "wiped" device with no words. Worker namespaces live in the shared `DATA_PREFIXES` const (nuke + dashboard walk it) — extend THAT list for any new worker storage namespace.

**Why:** device_secret is per-device, so a second device for one handle needs a network-held, signed membership record.
**How to apply:** membership questions go thru fold(); never persist fleet state on the device as authority.
