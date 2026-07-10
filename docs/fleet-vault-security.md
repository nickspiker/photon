# Fleet & Vault Security — scoped keys, loaners, revocation, persistence

> Status: **DESIGN.** Arrived at in the 2026-07-04 design session by reasoning from the shipped fleet/fan-out primitives outward.
> The *encryption model* composes with what's built; almost none of the mechanisms below are implemented yet.
> Marked ⏳ where unbuilt.
>
> Companion to [keyring.md](keyring.md) (membership chain) and [device-lifecycle.md](device-lifecycle.md) (attest/add/remove flows).
> This doc is the **vault-access + revocation + persistence** layer that sits on top of fleet membership.

---

## The one idea everything falls out of

The per-member **fan-out** (shipped: `fanout/` on FGTW, a fleet key sealed to each member's device pubkey, BRAID v0.2 §14.2) is not just "deliver the fleet key." Generalize it to **deliver a per-device bundle of keys**, and every feature below is "which keys are in your slot."

- Full device → bundle = all keys.
- Scoped device (smart display, kid's tablet) → subset.
- Route/presence-only device (IoT switch) → empty bundle.
- **Loaner / paused → empty bundle.** Pause *is* "grant the empty set."
- Revoked → slot deleted.

Full / scoped / IoT / loaner / revoked are **not different systems** — they're different bundles in the same fan-out.
Tiered device authority (the patent's `devauthority`) becomes "what's in your slot," not a separate feature.

---

## Key hierarchy — rotate keys, not data

Three levels, so revocation is cheap and bulk data never moves:

```
domain data (messages, calendar, contacts, photos, work-chat…)
    encrypted under a STABLE per-domain DEK          ← never rotates, never re-encrypted
DEK
    wrapped under a KEK delivered in the fan-out      ← tiny (wraps a 32-byte key)
KEK
    sealed per-device in the fan-out bundle           ← this is what you add/remove/rotate
```

- **The vault is already domain-partitioned** (shipped: `vault_key(domain, scope) = blake3_kdf(domain ‖ scope)`), so per-domain scope keys drop in naturally.
- **Issue a scope** ⏳: seal that domain's KEK into a device's fan-out bundle.
- **Add a scope** ⏳: add one more KEK to the bundle, re-seal, post.
- **Remove a scope / revoke a device** ⏳: rotate the KEK (new KEK', re-wrap the tiny DEK under KEK', re-seal to remaining holders, bump epoch). **Cost = O(devices × scopes), a few dozen tiny seals — NOT O(data size).** Loan a device with 200k messages and the revoke is the same speed as with 3, because the messages never move.

### When you re-encrypt data (the expensive tier)

Rotating the KEK gives **"no future access" + "the local ciphertext on the revoked device is inert"** — because that device can no longer fetch the KEK to unwrap the DEK.
It does **not** defeat a DEK the device already **extracted and kept** (live-RAM extraction, or a device you now treat as hostile).
To scrub even extracted-key access you must rotate the DEK and **re-encrypt the domain** — O(data).

Reserve that for **compromise / remove**, never for a **loan**.
For a loaner — someone you trusted enough to hand the hardware to, with RAM-wipe covering the honest case — you pay only the cheap KEK rotation.
If you genuinely fear the borrower will cold-boot your DEK out of RAM, the problem isn't the crypto.

**The forward-secrecy dial lives at the DEK**, per scope: stable DEK = cheap revoke (light switch); rotate-and-re-encrypt = scrub extracted access (work-chat).
Your call per domain.

Bonus: device add/remove is **rare** (people don't churn devices) *and* now **cheap** (KEK re-wrap).
Rare × cheap = a non-event.

---

## Vault-at-rest is network-gated (the real upgrade)

Today the vault is encrypted with the fingerprint-derived device key alone, so anyone with the hardware who knows your handle re-derives the key and reads it (the README "the ID isn't secret" gap), and revocation "doesn't reach back" to on-device data.

Fix ⏳: **the KEK/VMK is fetched from the fan-out per session and held in RAM only — never persisted to the device's disk.** Then:

- Active member → fetches from its fan-out slot, unwraps with its device key, holds in RAM, works.
- **Paused/loaned/revoked device → excluded from the fan-out → can't fetch → local vault inert.** No wipe, no re-sync.
- Resume → re-added to the fan-out → fetches again → data springs back.

This makes **revocation retroactively protect at-rest data** — a removed device's local vault goes dark the moment the fleet stops fanning to it.
A big chunk of the PIPE gap closed using the fleet as the secret-holder, no silicon required.

### The three layers of "keep the hot key safe" (do-now)

Hot-secret RAM handling — `zeroize` (compiler-elision only) + `mlock`/`VirtualLock` (no swap) + disable core dumps + copy discipline (fixed arrays, no clone/move).
Structurally **not** closeable in userspace: hibernation (dumps all RAM regardless of mlock) and live-RAM/cold-boot extraction — those are the PIPE / enclave line.
Claim "best-effort scrubbed and unswappable," not "guaranteed gone." **ferros must close these at the OS layer** (write-only key region, never pageable, never in a hibernation image).

### Honest edges

1. **Online-to-unlock.** Cold start needs one reach to the web to fetch the slot; RAM-cache for the rest of the session. Offline cold-start would need a device-disk cache (reopens the hole, device-key-decryptable) or a user PIN (reintroduces the secret) or PIPE. So no-secret + at-rest-protection ⇒ "must be online to begin." A short grace cache is the dial; be clear-eyed it's a hole for its lifetime.
2. **Theft window.** A device stolen while a session is unlocked can be read until you revoke; revocation locks it going forward. Same shape as remote-wipe — works once triggered.
3. **Live-RAM extraction** is the residual → PIPE.

---

## Where the key actually lives (persistence & availability)

The key does **not** live only in device RAM.
It persists in the **fan-out on the web, sealed per-device (zero-knowledge)** — FGTW today, the peer trust web tomorrow (peers-are-FGTW, ⏳; fgtw.org as bootstrap/backup).
Three homes, blind at every one:

1. **Your own fleet** — live copy while any device is on. With always-on bound devices (phones ~never off, IoT never off), this is a **standing mesh that's essentially always live**, so the layers below are tail-risk, not the common path.
2. **The web (peers / "not-friends")** — sealed operational copies; peers store ciphertext they can't open. Survives all-your-devices-off; revocable by pulling a slot.
3. **Custodians (friends)** — social-recovery shards, K-of-N, for the "web lost it AND every device gone" catastrophe. Human-threshold *recovery*, distinct from routine storage. The only layer that rebuilds you from nothing. (⏳, deferred behind device-sync.)

**Storing on the web's disk (per-device-sealed) instead of the device's own disk is the whole mechanism**, because it's both *survives-all-off* and *revocable* — a device-disk copy is neither (a thief re-derives the device key).
Device-disk only buys offline, at the cost of revocability.

### Multi-node query: integrity vs availability

Rotation = one authorized writer re-seals + posts with a bumped, signed **epoch**; every reader pulls the newest it can verify.
Querying N nodes (the nunc-time "42 sources" shape) works differently here than for time:

- **Time needs consensus** — no cryptographic "correct time," so take the median, average out liars, need a majority honest.
- **Key/state needs only availability** — there *is* a cryptographic correct (the sealed blob at the highest signed epoch), so take the **newest valid**. You need **one honest, reachable node with the latest copy.** The other 41 can be stale or malicious.

So N-node spray buys **liveness, not trust**.
The triad to write on the wall:

> **They can't deceive you, they can only deny you, and denial has a fallback.**

- **Integrity survives 41 of 42 liars** — the seal won't open if tampered, the epoch can't be forged higher. Never a forgery.
- **Availability degrades gracefully** — a withholding majority or an eclipse can deny you the newest (stale-but-valid) or anything, never a lie. Falls thru to: your devices *remember the highest epoch seen* (rollback-proof once you've seen fresh; the eclipse only bites a cold-start into a fully-hostile view), then custodians.

The more nodes you spray, the more *available* it gets without getting less *trustworthy* — because the seal and epoch carry integrity so the nodes are just shelves.
Most distributed systems can't separate those two; this one does.

---

## The loaner flow (guest mode) ⏳

A loan is **NOT** a membership op on your fleet.
Your device stays in your fleet the whole time; the guest is a separate identity coexisting on the same hardware (patent Fig 3, "one device → many handles, by the owner's grant").
Your fleet chain is never modified — so no re-pairing, no owner-side membership shuffle.

**Three problems, separated:**
1. **Coexistence** (don't erase my stuff) — free: vault entries are seed-scoped; the guest writes a different scope, erases nothing.
2. **Access control** (just them / open) — an owner-signed grant policy on the device: whitelist a handle_proof, or "open guest." Checked at the guest's attest gate. (The patent's owner-gated, off-by-default plurality.)
3. **Isolation** (guest can't read my data) — **not** solved with a secret; solved by the network-gated vault: on loan you **pause** (fold out of the fan-out), your KEK/VMK leaves RAM (zeroized), and your local vault goes inert with no wipe. The guest can type your handle and get nothing — no key to fetch, nothing to decrypt.

**Flow:**
- **Loan:** owner session → "Loan this device" → scope (a handle / open) → owner's vault pauses (fan-out fold-out + RAM zeroize). *Your fleet: untouched.*
- **Guest use:** friend attests → grant check → guest session; to actually *use* messaging they temp-enroll the device into **their** fleet (one enroll+rotate in *their* fleet, not yours). *Your fleet: still untouched.*
- **Return:** guest signs out; owner resumes (re-added to fan-out → vault springs back, no re-sync); optionally revoke the grant. *Only shuffle was in the borrower's fleet, where it belongs.*

"Erase unless you trust the guy" becomes optional belt-and-suspenders, not the mechanism.

---

## Device-management invariants ⏳

- **No self-remove.** A device cannot remove itself from the fleet. Two independent reasons: (a) removing the *last* member folds the chain to **zero members** — forward-extension-only, no prior member to authorize a re-add → **permanently bricked identity**; (b) a thief could have a stolen device self-remove to *launder* it clean for resale before you revoke it. One rule kills both. Removal is a "manage my *other* devices" operation (which is also the real security shape — you revoke a device you no longer hold, inherently from a different one).
- **Never zero members.** The client refuses to build any Remove that empties the fleet; the worker rejects a posted chain whose head folds to zero. Same belt-and-suspenders as the genesis-CAS and canonical-handle guards.
- **Local self-directed ops** are Sign out (clear session registers) and Erase (+ wipe local vault) — never a fleet Remove. See the naming below.

### Terminology (skip "de-attest" — it's insider jargon)

- **Sign out** — clear the session registers (identity_seed, vault_seed, handle_proof). Vault stays encrypted on disk; re-attesting the same handle re-derives and resumes. Reversible. *(This is what "clear the local seed" is — NOT de-attestation.)*
- **Erase** — sign out + wipe the local vault. Data gone locally; the network identity still exists and is re-derivable.
- **Remove this device** — the fleet unbind + rotate. The only op that changes the world's view; the real "de-attestation," normally done from *another* device.

**Determinism caveat for Sign out / Erase:** because the device key is fingerprint-deterministic, neither relinquishes the identity — re-attesting resurrects it (the ops-trap).
UI must say so: *"This clears this device.
Your identity still exists — re-enter your handle to return.
To truly leave, remove this device from your fleet."*

---

## Status ledger

- **Shipped:** fleet chain, pairing v1, per-member fan-out (fleet key), fstate.
- **Designed here, unbuilt (⏳):** scoped-key bundles + KEK/DEK split, network-gated vault-at-rest, pause/resume op (loaner + tiered devices + revocation, one primitive), guest-mode, no-self-remove + never-zero-members, secret-memory hygiene, peers-are-FGTW node spray, social-recovery custodians.
- **PIPE endgame (the honest residual everywhere):** live-RAM/cold-boot key extraction, hibernation, global owner-of-record anti-theft binding, per-identity hardware-sealed keys. Software raises the bar a lot; silicon draws the floor.
