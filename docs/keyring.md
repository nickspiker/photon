# The Keyring — multi-device identity for Photon

> Status: **DESIGN ONLY, not implemented.** Agreed shape from the 2026-06-29 design session.
> Large, foundational, security-critical — build it deliberately.
>
> **AUTHORITY: the TOKEN patent already specifies this** (`/mnt/Octopus/Code/TOKEN/patent/patent.typ`).
> The keyring is the patent's **device-binding** model; this doc records how Photon implements it.
> Where this doc and the patent differ, the patent wins.
> The hand-rolled "hash-linked log" sketch in an earlier draft is SUPERSEDED by the patent's registry-of-bindings model (below).

## What the patent already decided

- **The device-set lives in the web (FGTW → peers), not on-device.** "the web, and not the device, is where the binding of a device to a handle is kept." Matches [[project_peers_are_fgtw]].
- **Each device binds via `binding_id = one_way_fn(device_secret ‖ handle)`** (claim `binding`). The OWF is **provably lossy** (claim `lossy`): ~`1−1/e` of the output reachable, the rest with no preimage.
- **Bindings are mutually unlinkable** — "neither reveals the device identifier, and neither reveals or has any relation to the other." So an observer who sees two binding_ids **cannot tell they belong to the same handle, nor count them.** This is what resolves the device-count-privacy question: the count is hidden *by construction* — **no Merkle tree, no accumulator, no multi-sig, no RSA/modulus needed.**
- **Add** = a new binding appears in the web (genesis = first device claims the handle, first-come, same as the handle itself). **Remove** = the binding is deleted from the user-controlled registry (claim `bindingrevoke`: removing one leaves every other binding and the credential unaffected). Revocation sticks even though the removed device still physically holds its secret because the web won't *honor* the deleted binding (rather than the device forgetting).
- **Two ORTHOGONAL relationships — do NOT collapse them into "one device, one handle" (that misreading forbids the patent's own Figure 3):**
  - *One handle → many devices* (THE FLEET — what the keyring is): unbounded. Your phone, laptop, watch, car all bound to your one handle. Claim `cardinality`: "invariant under the number of devices."
  - *One device → many handles* (a shared/lent device): allowed but **owner-gated and off by default**. Claim `binding` / Figure 3: "A single device may hold a plurality of valid bindings... for several handles." Line ~735: "the plurality-of-bindings property is available **by the owner's grant** and absent by default."
  - The patent's actual web invariant (line 741-742) is an **anti-theft** rule, NOT a cardinality cap: a device has one *owner of record*, and a **foreign/unauthorized** handle cannot **displace** that owner's binding without the owner's authorization. It does not forbid the owner from binding additional handles, and it places no limit on the fleet. Read it as "a foreign handle can't squat your device," never "a device can hold only one handle."
- **Verification:** a device proves membership by an **attestation signed by its device secret** (claim `machine`: "produce attestations signed by the device secret without exposing it"), checked by the web / sibling devices against a fresh challenge (anti-replay). The verifier never sees the secret.
- **Device authority is tiered** (claim `devauthority`): asserting the credential ≠ authority to enroll/revoke devices. A device may be "fully the user" yet hold no fleet-management privilege. Remote revoke by a management-authorized device, or by a custodian threshold if the only authorized device is the one lost (claim `devrevoke`). Every assertion is broadcast to the whole fleet by default (claim `assertionalert`) — the "loud UI on change" requirement, already a patent claim.

## The implementation reality — the lossy OWF IS built (corrected 2026-06-29)

The provably-lossy OWF **exists and is used everywhere**: it is `ihi::chaos_amp` (the 32-op data-dependent ALU, `ihi/src/chaos_amp.rs`), wrapped by `ihi::spaghettify` (`ihi/src/spaghettify.rs`).
It matches the patent `lossy` claim point-for-point: data-dependent op selection from a 32-op menu (`val[4:0]`); 11 lossy + 3 extreme-lossy ops (POPCNT, SAT_ADD/SUB, PCNT_REPLACE) that destroy bits *within* the rounds and compound across them; ~10^482 op-selection paths (> atoms^2); ~700–2500 cumulative bits destroyed per call.
Crucially the data-dependency governs *op selection*, not memory access — the exact thing the patent distinguishes from memory-hard functions.
`chaos_amp` is **bit-exact with PIPE silicon** (`/mnt/Octopus/Code/pipe/rtl/chaos_amp_v2.v`).

(Earlier in this session I twice mis-identified `ihi::handle_proof` — the **memory-hard PoW**, the *anti-squatting cost* primitive, a different function — as "the lossy OWF, not implemented." Wrong on both counts.
`handle_proof` is the cost half; `spaghettify`/`chaos_amp` is the lossy half.
Both patent primitives, both built.)

**Consequence for the keyring:** the information-theoretic device-count unlinkability is available **today**.
A device binding `spaghettify(device_secret ‖ handle)` is provably-lossy unlinkable now — no "ship BLAKE3 now, version-bump later" compromise, and no need for Merkle/accumulator/multi-sig/RSA.
Use `spaghettify` for the binding derivation from the start.

## Auth rule (decided 2026-06-29): any single current member may Add and Remove

Reconciled with the patent: the patent's revocation is "delete the binding from the user-controlled registry," authorized per claim `devauthority` by a management-privileged device (or custodian threshold if that device is the one lost).
"Any single member" = any member holding the enroll/revoke privilege.
The patent's tiered-privilege model is the refinement: not every member need hold that privilege.

## The problem

Photon's identity is a per-device secret derived from the OS device fingerprint and **never stored or transmitted** (`device_secret = BLAKE3(oracle)`; see `docs`/`tohu`).
Everything keyed off it is therefore per-device:

- the FGTW network keypair (`derive(device_secret)`),
- the avatar write-auth key (`BLAKE3(device_secret || handle_hash || "handle-avatar")`),
- the vault encryption key.

So a **second device for the same identity** (same handle) derives a *different* `device_secret`, hence a different avatar-auth key — and FGTW's `avatar_put` rejects it with `403 "owned by different identity"`.
To FGTW and to peers, your own new phone is a stranger who happens to know your handle.
The avatar slot is just where this surfaced first; the same wall stands in front of every device-keyed write (network identity, vault, contacts-sync).

The fix is not "share the secret across devices" (you can't un-share a secret, so you could never *remove* a device) and not "store the keypair to disk" (that creates an exfiltration surface the current model deliberately lacks).
The fix is a **keyring**.

## Model (per the patent — supersedes the earlier hand-rolled "signed log" sketch)

A keyring is the identity's **set of device bindings held in the web**, each `binding_id = one_way_fn(device_secret ‖ handle)`, mutually unlinkable so the set's size is hidden.
Each device keeps its own `device_secret` (the "never leaves the device" invariant holds); membership is proved by a device-secret-signed attestation against a fresh challenge, never by revealing the secret.
Every device-keyed write (avatar, vault, contacts-sync) stops asking "is this *the* device_secret?" and asks "**does this writer hold a current binding for this handle?**" — answered by an attestation the web verifies, not by reading a published list.

The genesis / add / remove / authority / loud-alert rules are all in "What the patent already decided" above.
The earlier draft modelled this as a signed hash-linked Add/Remove log with deterministic fork-resolution; that is one possible *implementation* of the registry but is **not** how the patent frames it (registry-of-bindings, web-enforced anti-theft owner-of-record rule, delete-to-revoke).
Build to the patent.
If a log is used internally for ordering, it's an implementation detail beneath the binding-registry semantics, not the user-facing model.

### Binding derivation

Use `ihi::spaghettify(device_secret ‖ handle)` (the provably-lossy OWF, already built and silicon-mirrored) for the binding id.
This gives information-theoretic unlinkability — device count hidden from everyone, including friends, for free — so the chosen "hide count from public + FGTW" tier is *exceeded* at no extra cost.
No separate count-hiding structure (Merkle / accumulator / multi-sig) is needed; the lossiness is the mechanism.

## Where it touches the stack (build order TBD)

- **tohu** — device identity already lives here; the keyring is the natural home for "which device pubkeys are this identity."
- **photon** — the log type, the fold/validate function, gossip carrier (phonebook-style anti-entropy), the loud-change UI.
- **FGTW** — `avatar_put` (and every keyed write) verifies the writer against the folded keyring instead of a single stored pubkey. The avatar slot's `avatar_auth/<key>` becomes a *set* / a pointer to the current keyring rather than one frozen pubkey. (Today: `fgtw/src/lib.rs handle_avatar_put`, the stored-pubkey-equality gate ~line 2063.)
- **gossip** — the log rides the same anti-entropy path as the phonebook (`peers-are-fgtw`); it's small and signed.

## Relationship to existing work

- **avatar exchange** (commit `d43b3ad`) exposed the problem: FGTW's first-writer-owns avatar gate (`avatar_auth/<key>` pubkey-equality) is exactly the single-device assumption the keyring generalises.
- **[[project_peers_are_fgtw]]** — the keyring gossips on the same mesh; device-sync was already the deferred home for this.
- **[[project_vault_roadmap]]** — device-sync (and thus the keyring) was explicitly deferred behind it.
- **[[custodes]]** — recovery, not operation; meets the keyring only at re-genesis.
