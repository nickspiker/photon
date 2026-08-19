# Fleet identity binding — remediation proposal

Status: **proposal, awaiting approval.** No code or comments changed yet.

## TL;DR

The ed25519 "identity binding" on a fleet genesis (`identity_pubkey` / `identity_sig`)
is presented in comments as proof that the founder *owns* the handle. It is not. The
identity key is `Ed25519(BLAKE3(handle))` — a deterministic function of the **public**
handle — so anyone who knows the handle can reproduce it. The binding is cryptographically
inert for its stated purpose. This document proposes making the code honest about that,
in tiers, without breaking any deployed fleet.

The fleet's *real* security is untouched by all of this and is sound: device-secret-gated
membership, TOFU genesis-hash pinning, and device-secret vault encryption at rest.

## Background: what the binding is

A fleet's membership chain begins with a genesis `FleetOp` that carries:

- `device_pubkey` — the founding device, which self-signs (`signer_pubkey == device_pubkey`).
- `identity_pubkey` — `Ed25519(identity_seed).verifying_key()`, where
  `identity_seed = Handle::to_identity_seed(handle) = ihi::handle_to_hash(handle)`
  (a plain BLAKE3 hash of the handle — cheap, `src/types/handle.rs:28`).
- `identity_sig` — a signature over the op's `signing_bytes` by that identity key.

`fold()` requires the genesis to carry a valid identity binding
(`verify_identity_binding`, `fgtw/src/fleet.rs:322`), and `genesis_identity_matches(seed)`
(`fgtw/src/fleet.rs:406`) checks `identity_pubkey == Ed25519(seed)`.

## Finding: the binding proves nothing about ownership

`identity_seed = BLAKE3(handle)` is a public function of a public string. Therefore
`Ed25519(BLAKE3(handle))` is computable by anyone who knows the handle, and
`genesis_identity_matches` returns true for *any* device that typed the handle. The
comments calling `identity_seed` "the handle's **secret** preimage"
(`fgtw/src/fleet.rs:187`) and saying the co-signature proves the founder "**owns** the
handle" (`fgtw/src/fleet.rs:418`) are false: it proves only *knowledge of the handle
string*, which is the premise, not ownership.

### Why no cleverer derivation fixes this

Every alternative that keeps the handle as the sole input fails for the same reason:

- **`handle_proof`** (the 24 MB / 17-round memory-hard PoW, `ihi/src/handle.rs`) is already
  the fleet's KV slot key — it is *published on the wire*, zero cost to obtain. Gating on it
  proves nothing.
- **`spaghettify(identity_seed)`** — `spaghettify` is a fast mixing hash, explicitly **not**
  memory-hard (`ihi/src/smear.rs:13`). It is just another cheap deterministic function of the
  handle. Swapping in the memory-hard function instead merely recomputes `handle_proof`.
- **General principle:** no deterministic function of a public input can produce a secret.
  Public handle in → public value out, for every `f`. Memory-hardness only *rate-limits*
  blind enumeration of many/unknown handles; against a targeted, known handle the attacker
  pays the cost once. This is information theory, not a fixable weakness.

The only way to make identity unforgeable is a **secret input that is not the handle**
(device oracle secret, passphrase, or a user-held random key with a backup story). That is
"Proposal B" below and is out of scope for this remediation.

## What actually secures the fleet (unchanged by this proposal)

- **Membership:** adding a device requires a sponsor already in the folded member set
  (`SignerNotMember`, `fgtw/src/fleet.rs:328`), signing with a device key derived from the
  machine fingerprint oracle — never from the handle. Handle knowledge cannot enroll a device.
- **Identity recognition:** friends pin the genesis hash at first fold (`pinned_genesis`,
  `src/types/contact.rs:271`); a different-genesis chain under the same handle is marked
  `wrong_identity` and refused. This TOFU pin — not the identity signature — is the real anchor.
- **Data at rest:** the vault is encrypted under the device fingerprint secret, not the
  handle (`FlatStorage::new(app, vault_seed, secret)` with `secret = device_secret`;
  `src/network/handle_query.rs:658`). The handle-derived `vault_seed` is only an address.

The one deliberate place the handle *is* the key is the submitted diagnostic logs — intended
and out of scope here.

## Proposal

Three tiers. Tiers 0 and 1 are the recommended scope; Tier 2 is deferred.

### Tier 0 — Truth pass (zero risk, highest value)

Fix every comment that claims the identity binding proves ownership. No code, no wire change,
no re-attestation. Sites:

- `fgtw/src/fleet.rs:187` — "the handle's secret preimage" → a public BLAKE3 image of the handle.
- `fgtw/src/fleet.rs:418` — "co-signs to prove the founder owns the handle" → proves only
  knowledge of the handle string.
- `fgtw/src/fleet.rs:135` & `:142` — "only the handle's owner can enter the set" → "only
  someone who knows the handle string can post a *pending* request; membership still requires
  a member sponsor." (Reclassify the bindreq `identity_sig` as an anti-spam gate, not auth.)
- `fgtw/src/client.rs:229` — "pins it to OUR identity" → "confirms the chain sits at our
  `handle_proof` slot; not an ownership proof."
- Audit the `src/network/handle_query.rs` comments around the `genesis_identity_matches`
  calls (`:465`, `:587`) for the same overclaim.
- `src/types/contact.rs:262` / `:271` are already honest — use them as the model.

State the real trust model once, plainly: *identity is a public, memory-hard-rate-limited
image of the handle; assurance comes from the TOFU genesis-hash pin and device-secret-gated
membership.*

### Tier 1 — Collapse the own-fleet check onto `handle_proof` (optional simplification)

Swap the own-fleet identity check from the ed25519 equality to the `handle_proof` match it is
really standing in for.

- Add `MembershipBlob::genesis_handle_proof() -> Option<[u8;32]>` (mirror of the existing
  `genesis_identity_pubkey`, `fgtw/src/fleet.rs:412`).
- `fgtw/src/client.rs:238` `current_members_verified`: replace
  `genesis_identity_matches(identity_seed)` with "the fetched chain's genesis `handle_proof`
  equals the `handle_proof` we queried by." Drop the `identity_seed` parameter. This is the
  actual anti-relay-swap TOCTOU check the function exists for.
- `src/network/handle_query.rs:465` & `:587`: same swap in the probe / attest verdict.
- Update callers whose signature changes: `src/ui/photon_app/devices.rs:800`, `:870`, `:2008`.

**Keep the bindreq `identity_sig`** as-is (it is a genuine handle-string anti-spam gate that
`handle_proof` — public — cannot replace; replacing it would be a downgrade). Tier 0 fixes only
its comment.

**Keep `fold` and the genesis builder as-is.** Old chains keep their binding and keep folding;
new chains keep emitting it. Tier 1 changes only what *callers trust*, not the wire.

### Tier 2 — Wire-format removal (deferred; flag-day only)

Remove `identity_pubkey` / `identity_sig` from `FleetOp` (and the bindreq `identity_sig` if it
is retired). Hard constraint: **genesis is immutable and chains are append-only, so the v0/v1
verify path cannot be deleted without forcing every existing fleet to re-found.** Two routes:

- **Flag-day v2:** bump `SIGNING_DOMAIN` (precedent: the v0→v1 sovereign-records break already
  did exactly this — `fgtw/src/fleet.rs:109`, "flag-day, v0 chains don't fold"). Clean end
  state, brutal migration.
- **Dual-path:** emit v2 without the fields, keep v1 verify indefinitely for legacy chains. No
  forced migration, but the code never shrinks.

Recommendation: **do not do Tier 2 on its own.** Tier 0 captures nearly all the honesty value
and Tier 1 the surface reduction. Piggyback Tier 2 onto the next flag day if one occurs for
another reason.

## Behavior-change register

One observable change, from Tier 1 only:

- Today, a squatter who founds *your* slot with a **random** identity key classifies as
  `Taken` (because `genesis_identity_matches(your_seed)` is false). After Tier 1 it classifies
  as `JoinOurs` (genesis `handle_proof` matches your slot). This only ever caught *lazy*
  squatters — a real attacker uses the canonical `Ed25519(BLAKE3(handle))` (they know the handle
  too) and already classified as `JoinOurs`. Either way you are locked out of an occupied slot,
  so the security outcome is identical; only the UX label moves. Documented here because it is a
  real, if negligible, difference.

## What must not change (the real security)

- Device-secret-gated Add (`SignerNotMember`).
- TOFU `genesis_hash` pinning.
- Bindreq `identity_sig` anti-spam gate (Tier 0/1).
- Vault-at-rest (device secret) and log (handle seed, deliberate) crypto.
- Wire format (Tier 0 and Tier 1).

## Test plan

- Existing `genesis_identity_binding_holds_and_matches_seed` and
  `swapping_identity_pubkey_breaks_the_device_sig` stay green (Tier 0/1 touch neither `fold`
  nor `signing_bytes`).
- Add `genesis_handle_proof_matches_slot`: the accessor returns the genesis op's `handle_proof`.
- Add a relay-swap test: `current_members_verified` rejects a fold-valid chain whose genesis
  `handle_proof` differs from the queried slot.
- handle_query classification: probing an occupied foreign slot still returns a coherent verdict
  under the `handle_proof` check (and the `Taken`→`JoinOurs` shift above is asserted, not
  accidental).

## Out of scope — Proposal B (real secret identity)

If identity must ever be cryptographically unforgeable, root the identity key in a secret not
derivable from the handle (device oracle, passphrase, or user-held key with backup). Its
apparent "recoverability cost" is largely illusory: data is already device-secret-encrypted and
fleet-key-gated to join, so the handle only ever bought addressing. Recorded here as the only
real fix, deliberately deferred.

## Recommendation

Ship **Tier 0** now (the fix for the actual objection: code that lies about what it secures),
and **Tier 1** in the same pass if a smaller crypto surface is wanted and the `Taken`→`JoinOurs`
UX shift is acceptable. Defer **Tier 2** to a flag day. Keep the bindreq gate.
