# The global vault — one store for all TOKEN apps

One vault per identity per device, shared by every TOKEN app, with entries addressed by domain-scoped keys — replacing the per-app vault files.

Status: DESIGN. Joins `docs/updates.md` and `docs/disclosures.md` in the design set; nothing here is built yet.

## Why

The stack already converged on this shape without naming it.
Vault addressing is de-stringing to `blake3(domain, scope)` (the storage-layering work), which means the *entry key* carries the app separation — at which point the `app_id` baked into each vault *filename* (`tohu::vault_path_name`) is redundant.
One identity → one data home: device-add, backup, and fleet sync move one thing instead of N per-app files, and cross-app user state (settings, theme, preferences) gets a natural home instead of each app inventing its own sidecar.

Timing: this is the cheapest it will ever be.
The vault has a handful of users, the keyring re-attest path makes migration nearly free, and the domain/scope de-stringing is mid-flight — the global container is a redirect of in-progress work, not a rework.

## Not the capsule, not custodes

Three objects keep getting conflated; the distinction is load-bearing, so once and for all:

| | Contents | Key | Survives | Syncs | Who holds it |
|---|---|---|---|---|---|
| **Session capsule** | the 96-byte roots (`identity_seed ‖ vault_seed ‖ handle_proof`) | boot-locked *wairua* (`spaghettify(boot_id …)`) | app restart / same-boot reinstall; **dies at reboot by design** | **never** — a synced capsule is dead ciphertext elsewhere, which is the security model working | this device only |
| **Vault** (this doc) | the actual data — contacts, chain state, settings, history | vault anchor key (`handle_seed ‖ device_secret`) | durable | **fleet** — the whole point of device-sync | your devices |
| **Custodes** | recovery-secret-wrapped identity material | recovery secret (never on any device) | durable | **opted-in custodians** (friends) | people you chose |

The rule that generates the table: **sync-worthiness equals non-derivability.**
The capsule's contents re-derive from the typed handle (never worth syncing); the vault's contents derive from nothing (always worth syncing); custodes bridges the lost-everything gap (syncs only to chosen humans).
Capsule = keys, vault = data — merging them either breaks the power-rail boundary or makes your data die weekly.

## Naming

One vault file per identity per device, name derived — the existing `tohu::vault_path_name(app_id, handle_seed, device_secret)` with the `app_id` fixed to a single stack-wide constant instead of per-app.
Deriving from `handle_seed ‖ device_secret` keeps filename opacity (probing whether a handle has used a device requires the device's secret) and makes multi-identity-per-device separation automatic: a shared tablet gets one vault per handle, and the attest-press fast path below opens the right one the moment the handle is typed.
A literal `vault.vsf` would lose both properties for no gain — nothing needs to locate the file without already holding the inputs.

## Addressing

Entries are addressed `blake3(scope ‖ domain-tag)` — the existing `vault_key(domain, scope)` idiom, with the identity seed as the scope for identity-wide entries.
The canonical example, user settings: `blake3(identity_seed ‖ VSF::a("settings"))`.
Domain tags are VSF `a` (ASCII) values, so the address derivation is spec-typed rather than bare string concatenation.
Per-app entries use the same idiom with an app domain in the tag; the address layout is flat — no tree, no path strings — exactly what the de-stringing work already established.

An address being derivable is harmless by construction: the address is only the lookup key.
Contents are sealed under the device-bound vault anchor key (`handle_seed` + `device_secret`), so someone who knows a handle can compute where an entry would live but can never read it.

## The attest-press fast path

Everything local is cheap; only the handle proof is expensive.
`identity_seed` is `handle_to_hash` (microseconds), the vault anchor key is one BLAKE3 over `handle_seed ‖ device_secret` (microseconds) — the ~1s memory-hard `handle_proof` is the ONLY slow step, and it is network-facing, not storage-facing.

So the moment the user presses attest:

1. Derive `identity_seed` from the typed handle — instant.
2. Derive the vault key, look up `blake3(identity_seed ‖ a"settings")` — instant.
3. **Pass** (entry decrypts): apply theme and preferences immediately; the app dresses itself in this identity's colours while the proof grinds. **Fail** (absent / undecryptable): defaults.
4. Only then does the handle proof spin up and the network attest proceed.

The pass/fail doubles as a free local "have I been here before" probe: a decrypting settings entry means this identity has used this device, with no network round-trip and no proof paid.
In-session, settings changes apply in RAM immediately and persist to the entry behind the interaction — vault storage never gates on-the-fly changes.

## Settings: per-device maps + link-to-global

Settings are NOT split into hardcoded "synced" vs "device-local" categories — that taxonomy is wrong.
Every setting is per-device, and each setting carries a per-device **link bit** choosing where its value comes from:

- **Linked** (to the global): the device follows the fleet-wide global value for that key, and adjusting the setting from ANY linked device writes the global — the change propagates to every device that has the key linked.
- **Unlinked**: the knob is set locally on this device; the global stops applying to it until the user re-links.

**Every setting is born linked** — the default is always "go with the fleet"; unlinking is the explicit per-device act for the knob that genuinely differs on this machine (display calibration being the obvious customer).
The canonical example — and what surfaced this whole design — is the Automatic-updates checkbox from `docs/updates.md`: it is just a settings key like any other, born linked, so flipping it flips the fleet, and a device you want on manual updates gets that key unlinked.
**Groups come later**: an intermediate layer between device and global (link-to-group), same resolution logic — the link becomes a target (local / group / global) rather than a bit; design the schema so that's an extension, not a rework.
(**Lock** — preventing adjustment, as distinct from linking — is a separate future concept and deliberately absent from this schema.)

Data model (rides the existing fstate transport, same as the roster):

- **Global layer** — one fleet-wide map `{key → (value, updated, tombstone)}`, merged last-writer-wins per key on the eagle-time stamp (exactly the `RosterEntry` merge shape).
- **Per-device layer** — one map per device `{key → (value, updated, linked)}`, authored ONLY by that device.
  Single-writer means no merge conflicts are possible within a device's map — a sibling just adopts the copy with the newest stamp.
- **Effective value** on device D for key k: `linked ? global[k] (falling back to local if the global is absent) : device[D][k]`.

Every device's map syncs fleet-wide, so any device can SEE every sibling's settings — which is also what feeds the Fleet management page (view a sibling's config from wherever you are; remote *editing* of another device's map waits for groups).
The sync payload is the global map plus the N device maps in photon's fstate slot, sealed under the fleet key, bumped and pulled exactly like the roster.

### Fleet page orbs (per-device avatars)

Each device row on the Fleet page gets an orb/avatar the user can set per device, drawn with the online/offline ring the contact rows already use (`is_online` on the sibling contact; the self row is always online).
Placement in the schema: device orbs live in the **global layer** keyed by device (`device.orb.<pubkey>`), NOT in the device's own map — deliberately, because you set a device's orb from whatever device you're holding, and the per-device maps are single-writer.
Any device writes any orb key; LWW merges concurrent edits like every other global key.
Value budget: the orb rides the fstate blob, so keep it small (a compressed thumbnail or a colour/emblem); if orbs outgrow the blob, the value becomes a hash reference into blob storage — the schema doesn't change.

## What stays outside the vault

Almost nothing — a deliberately thin plain layer for knobs needed before any handle exists:

- `main.rs`-time dev knobs (the log hex-elision head/tail load before any identity is possible).
- Arguably device update policy on a never-attested machine — though compiled defaults ("auto-update on") cover that until a handle shows up, at which point the identity's own settings take over.

The old framing ("identity-scoped → vault, device-scoped-pre-identity → plain file") survives, but the fast path shrinks the plain file to a stub: hand-editable dev knobs, nothing a user touches thru UI.

## Per-domain sealing inside the container

One file does not mean one key.
kete already derives a per-entry ChaCha20-Poly1305 key, so domain scoping is nearly free: a calendar app's domain keys cannot open photon's message domain, and one deliberately-shared `user` domain (settings, theme, prefs) is readable by every stack app.
This matters the day TOKEN apps aren't all written by one person — "same user installed it" must not mean "it can read your messages."

## Platform reality: the API is global, the file is not

**Desktop:** genuinely one file.
Keep the filename derived (`vault_path_name` minus the app_id) rather than a literal `vault.vsf` — that preserves filename opacity (can't probe whether a device has a TOKEN vault) and multi-identity-per-device separation for free.

**Android:** the app sandbox makes a literally-shared file impossible — app-private storage is per-app by OS design, and the calendar APK cannot read photon's files, period.
So on Android the global vault is per-app replicas converging thru the fleet (the fstate sync machinery that already exists), with a content provider as a possible same-signer local optimisation later.
Which is the real lesson: **"global vault" is a kete API contract, not a shared path.** Desktop implements it as one file; Android implements it as fleet-synced replicas; callers cannot tell.

## The one hard problem: concurrency

Two stack apps running simultaneously on desktop means two processes writing one file — today's per-app files never contend, so this is genuinely new work.
The photon log already does flock-guarded trims (the pattern exists); a proper multi-writer store needs a real story: advisory lock + retry, or single-writer-per-record riding manifestus's dual rings.
This is the engineering cost of the whole design and should be prototyped first, before any migration.

## Sidebar: the VSF editor

A general VSF editor is wanted eventually (hand-editing spec'd files without hex).
It slots into toka (the wasm VSF inspector already deployed on fgtw.org) as view→edit, not a new tool.
Parked; noted here so it stops being re-derived.

## To build (rough order)

1. Concurrency prototype: two processes, one vault, flock + retry vs. ring-arbitrated writes — pick one with evidence.
2. kete: the global-container API (open by identity, read/write by `(domain-tag, scope)` address, per-domain key derivation).
3. The settings domain + attest-press fast path in photon (theme applies before the proof).
4. Migrate photon's existing per-app vault entries into the container (cheap now — handful of users, re-attest recovers).
5. Android replica convergence via fstate (later — desktop-first proves the API).
