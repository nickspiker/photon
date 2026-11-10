# The identity/device lifecycle — flow tree, screen names, conventions

> Status: REVISED 2026-11-08 for plate and papers (owner: "Write it! I think we distilled the idea quite well. Then build it!"); first agreed 2026-07-17.
> This is the canonical map of every state a device passes thru from first launch to final exit, the screens that render them, and the names we give them.
> Companions: [one-identity.md](one-identity.md) (one identity per device for every app, the About / Attached / Discover page), tohu docs/oracle-hardening.md (where each plate and key comes from), [pairing-v2.md](pairing-v2.md) (the add ceremony), [device-trust-and-recovery.md](device-trust-and-recovery.md) (consent-only removal, custodians).

## Why: a device key must be stable and secret

A device key has to be **stable**, so a reset device comes back as itself without the rest of the fleet doing anything, and **secret**, so nothing else can become it.

- **ferros has both, and is the reference.** Only the kernel reads the hardware root, and PIPE will answer challenges without ever revealing it. Reset a Pixel on ferros all day and reinstall: it walks back in as the same device, and nothing below applies.
- **Hosted OSes give one or the other, never both.**
  - The stable sources (Widevine, `machine-id`, `MachineGuid`, the TPM, `IOPlatformUUID`) can be read, or in the TPM's case used, by anything running on the device. Stable, not secret: today a desktop's key is rebuilt from its `machine-id` by any process, so the handle plus that file is the device, from any computer.
  - The secret sources (StrongBox, keys bound inside a TPM, the Secure Enclave) die with a reset. Secret, not stable.
- **So on hosted OSes the device carries two things** (owner, 2026-11-08: "leave the plate on each one, the best we can, then put the cert in the glovebox"):
  - **The plate:** the stable, public value. It names the hardware and proves nothing. fgtw.org ties each plate to the identity that claimed it (one plate, one identity), which keeps the anti-sybil rule (`feedback_device_root_never_salted`: one device, one plate, never salted).
  - **The papers:** a key the secure hardware makes and will not export, registered in the fleet chain. It proves the device, and a reset destroys it.
- **The re-tap after a reset is the platform's cost, not the design's.** The protocol treats papers and the ferros root as one thing, the device's key, differing only in assurance level. Nothing in the chain branches on which OS a device runs, and none of this machinery goes into ferros.
- **Ruled 2026-11-08: the papers overrule the keystore rule** ("I think we have to overrule for papers"). The rule still holds for the plate and for anything copyable; a key the hardware will not export is the exception, and using it locally puts no vendor in the loop.
- **What stays as today:** a plate is a statement, so a modified client can state any plate, and anti-sybil stays at the casual level until an answered source (PIPE, a certified TPM) proves one. Checking a vendor's attestation chain would prove it sooner but puts the vendor in the loop, so it stays out.

## Plate and papers per platform

| platform | plate (public name) | papers (non-exportable) | the device secret | what destroys the papers |
|---|---|---|---|---|
| ferros | the kernel's hardware root | none needed | the kernel's root (PIPE later) | nothing |
| Android | `BLAKE3(Widevine ID ‖ signing cert)` | StrongBox HMAC key (the TEE keystore where there is no StrongBox) | the key's HMAC over a fixed label | factory reset; uninstalling the app that holds it |
| Linux, Windows with a TPM | the TPM endorsement key | the TPM's HMAC primary | the TPM's HMAC over `machine-id` / `MachineGuid`, so it changes when the OS is reinstalled | OS reinstall |
| macOS with a Secure Enclave (T2, Apple silicon) | `IOPlatformUUID` | a Secure Enclave P-256 key, this-device-only (never in iCloud Keychain or a backup) | key agreement with a fixed public key: deterministic, 32 bytes | erase |
| no TPM, no Secure Enclave | `machine-id` / `MachineGuid` / `IOPlatformUUID` | none | as today (`BLAKE3` of the plate source) | — |

A device without papers stays at level 0 and is added by proximity only (rule P2 below).

**The root is a strict priority, never a choice** (owner, 2026-11-08: "don't think the user should be choosing"). Each platform takes the first source that answers:
- Linux, Windows: TPM papers, then the machine identity (built: photon `fingerprint.rs` `select_desktop_root`, tohu `DesktopRoot`).
- macOS: Secure Enclave papers (not built), then `IOPlatformUUID`.
- Android: StrongBox papers (not built), then Widevine ‖ certificate ‖ `ANDROID_ID`, then `ANDROID_ID` (the conversion pin).

A device never falls down the list once it has moved up it: a source that does not answer while the device's vault is under it stops the start, because falling through would make it a different device and the census would delete its vault (Android's reinstall trap, 2026-11-04). A TPM that is absent or that the user may not open answers the same every time and falls through on a device that never moved; any other failure is asked again before it counts.

To check: whether macOS lets a Developer ID–signed app make Secure Enclave keys without a provisioning profile; whether clearing an Android app's data kills its keystore keys (uninstalling does).

## The state model (what the screens render)

Four independent axes; every screen is a view of one combination.

**Device** (this hardware, as the fleet and fgtw.org see it):
- `virgin` — no papers here and the plate unclaimed: new hardware, or hardware whose plate was released.
- `owned(I)` — no papers here, the plate claimed by identity I: a reset (or, on Android, an uninstall) destroyed the papers. It joins no other identity and cannot sign itself back into I.
- `bound(I)` — papers here and registered in I's chain. THE RULE: **one identity per device, one-or-more devices per identity.**
- `locked(I)` — the plate is in I's `locked_plates` (declared stolen or lost). Refused under any papers, for every identity, until I unlocks it.

**Identity** (the handle's fleet chain, global):
- `unclaimed` — no chain at this handle_proof.
- `live` — the chain folds to ≥1 member device. It can never fold to zero (D3: identity never dies).
- `superseded` — custodian-quorum recovery replaced the chain (designed, not built: docs/device-trust-and-recovery.md).

**Session** (per boot, shared by every FGTW app on the device, one-identity.md):
- `signed-in` — the tohu registers are live; every app opens straight in.
- `signed-out` — a reboot cleared them; the handle is typed again in any app (Member resume, ~1 s proof, no new claim), and every app sees it.

**App** (per identity, the `kists` record, one-identity.md):
- `attached` / `detached`.

## Conventions

- **Flood states**: a whole-screen colour + matching orb tint marks an exceptional device state that must be recognizable across the room. Amber = development build (exists). **Green = "Selected!"**, the joining device lit by its sponsor's pick (with papers it is proof: only the picked papers can light). Floods are themes, not dialogs: the whole surface changes.
- **The interstitial pattern** (from the permanence warning, `LaunchState::Confirm`): an irreversible act arms a full explanation + a re-labelled button; ONLY pressing that button again proceeds; every other interaction — editing, tapping anywhere else, navigating — cancels. No timed anything (event-shown, interaction-cleared, per the no-time-based-UI rule).
- **Notices**: event-shown, interaction-cleared bands/toasts. Green band = confirmations ("Device added √"), amber = warnings (clock-off, degraded vault, "someone else knows your handle"), never timed.
- **Screen names** (canonical, use these in code comments + tickets): `Launch`, `Unbound` (Launch on a `virgin` device: "This device is unbound — tap to attest"), `Owned` (Launch on an `owned` or `locked` device), `Claim` (the permanence interstitial), `Attesting`, `KnownHandle` (D1), `JoinerWaiting` (posted, waiting to be picked), `JoinerSelected` (green flood), `SponsorAdd` (today `AppState::AddDevice`), `SponsorConfirm` ("did it turn green?"), `Ready`, `Conversation`, `Panel(page)`.

## The rules (owner, 2026-11-08: "build it")

- **P1 — a reset device belongs to its owner.** Its plate stays claimed by I; only I's tap (a bound sibling, or the owner's tag) adds it back, and it makes new papers in the act.
- **P2 — no papers, proximity only.** Without papers green is not proof (a key rebuilt from a public value lights a clone), so such a device is added only over BLE or NFC, never from the network list.
- **P3 — contention shuts the network path.** More than one request for the identity in the window: the sponsor offers only devices it can hear (BLE now, NFC later) until that add finishes, so withdrawing and reposting cannot reopen it, and says plainly that someone else knows the handle. Owner: "you'd just completely disable that option and require BLE or NFC presence, like a traditional device bind."
- **P4 — green is proof, so the words leave the network path.** With papers only one device can hold the picked key, so only one can light; the owner confirms on seeing it ("I'm not going to click add until I see it do so"). The 23 typed words stay only where a device has no papers and no proximity.
- **P5 — the same plate retires its own ghost.** When an `owned` device is added back, the old row with the same plate is retired in the same act. The new papers sign the add, so the hardware itself consents, which keeps removal consent-only (docs/device-trust-and-recovery.md). Lost hardware stays a ghost until locked.
- **P6 — the owner can release a dead device's plate alone,** so broken hardware can be sold or given away. A live device leaves in two signatures as today (its departure, then a member's release), and the release now frees the plate as well as the key.
- **P7 — on Android the papers belong to one holder app** on the device, which answers for the others (one-identity.md). Uninstalling the holder costs a tap; uninstalling any other app costs nothing.
- **P8 — the owner's tag is a credential, not a device.** A FIDO2 key with `hmac-secret` (tohu docs/oracle-hardening.md, "A tag as PIPE"), enrolled in the chain: it can sponsor a device back in, so a one-device owner who resets has a way home, and it can gate the start of a session. It replaces the 2026-07-17 backup tag (D3), whose flashed seed was a copyable bearer key.

## The flow tree

```
FIRST LAUNCH of any FGTW app
├─ session signed-in (another app attested)    → READY
├─ bound(I), signed-out                        → LAUNCH "Re-enter your handle" → Member → READY
├─ owned(I) / locked(I)                        → OWNED "This device belongs to another identity" — nothing else
│                                                 (the owner adds it back from a sibling, P1)
└─ virgin → UNBOUND "This device is unbound — tap to attest" → handle → PROBE
   ├─ Fresh     → CLAIM interstitial ("Yes — forever") → ATTESTING (genesis carries plate + papers) → READY
   ├─ JoinOurs  → KNOWN HANDLE (D1)
   │               ├─ "It's mine — add this device" → JOINER WAITING (request posted)
   │               │     └─ picked → JOINER SELECTED (green flood) → owner confirms → READY
   │               └─ "Pick another name" → LAUNCH
   ├─ plate claimed by another identity → OWNED
   └─ Taken     → error line → LAUNCH          (genesis/identity mismatch — near-theoretical)

SPONSOR side (a bound device, or the owner's tag):
READY → orb → PANEL(Fleet) → SPONSOR ADD
   ├─ one request, joiner has papers   → the network list; an owned device's own plate shows as "your <device> is back"
   ├─ joiner has no papers             → proximity only (P2)
   ├─ two or more requests             → proximity only until this add ends + amber "someone else knows your handle" (P3)
   ├─ NFC tap (physical proximity, the Fleet page open) → bind + key release as one act, like typed words (owner 2026-11-10)
   └─ touch on a BLE / local-network row → the joiner is told (bind_select), NOTHING bound → joiner lights green → distinct green "Add <name>" (bind + key release as one act); not green → Back, nothing to undo (owner 2026-11-10)
        └─ Add → bind + fleet-key rotation together; a same-plate ghost retired in the same act (P5) → "Device added √"

EVENTS on a bound device
├─ reboot                    → signed-out (every app) → re-enter the handle in any app (+ the tag, if it gates sessions)
├─ app reinstall             → ferros, desktop, Mac: resume │ Android: the holder's papers gone → owned(I) → owner taps (P1, P7)
├─ OS reinstall / reset      → owned(I) → owner taps back in → new papers, ghost retired (P5)
├─ lost / stolen             → owner locks the plate from a sibling → locked(I); a thief's reset leaves it locked(I)
├─ sell / give               → depart (the device signs out) + release (frees plate and key) → virgin
│                              device already dead → owner releases the plate alone (P6)
├─ hardware swap, fTPM reset → a new plate → new hardware; the old row is a ghost
└─ last device               → cannot sign out (D3: identity never dies)

OWNER CREDENTIALS (P8)
├─ tag enrolled  → sponsors a device back in; can gate the session start
├─ tag lost      → revoke it from a bound device; enroll the spare
└─ every device and every tag gone → custodian supersession (designed, not built)

EXITS (PANEL → Security, the destructiveness ramp — green→yellow→orange→red):
   Lock            — reversible; re-type the handle to resume.
   Depart          — self-signed fleet departure, then the identity material is wiped; refused for the last member (D3).
   Shred           — crypto-wipe WITHOUT departing; the device becomes owned(I), and P5 retires its row when it comes back.
   Remove & shred  — depart THEN wipe, gated on the departure landing.
```

## The move to papers (one migration per device)

Papers change what feeds the device secret, which tohu freezes at `v0` (oracle-hardening rule 7), so every device with papers moves once, the way Android moved onto the Widevine root on 2026-11-02 (photon `ui/photon_app/root_conversion.rs`, kete `FlatStorage::rehome_into`):

1. The device makes its papers and derives the new device secret beside the old one.
2. The old key, still a member, sponsors the new key into the chain and grows the fan-out to it (the 2026-11-04 fan-out fix), then pushes the chain to the siblings before anything else (the 2026-11-02 Kaitiakitanga fix).
3. The vault re-homes from the old secret to the new: entry addresses never mix the secret in, only the value keys do.
4. The old key departs; the plate carries over unchanged.
5. The live session carries over: Android seals its boot capsule under the new root before it restarts, and the desktop's registers are not sealed to a root at all, so a key move asks nobody for the handle (owner, 2026-11-08: "it SHOULD not ask to re-attest, roll keys, then regenerate the boot capsule").
6. The old root stays readable under a dated pin (`MIGRATION-EXPIRES`, `feedback_pins_are_dated`) so a device that has not moved yet still opens.

Devices without papers do not move. ferros does not move.

## D1 — the collision flow (KnownHandle)

**Defect**: a first-time user who types an already-claimed handle lands on "type these words to add this device". They think they're pairing; really they collided.
**Why it can't be "detected"**: knowledge of the handle string derives the identity seed; a collider's probe verifies the genuine chain's genesis as "identity-bound to this handle" — cryptographically indistinguishable from the owner's own new device. `ProbeOutcome::Taken` only fires on a genesis↔handle mismatch, which the derivation makes near-impossible. So `JoinOurs` IS the collision branch, and the screen must speak to BOTH readers.

**The KnownHandle screen** (replaces dropping straight into join words):
- Headline: **"This name is already claimed."**
- Two readings, two affordances, taken-reading first (the more common visitor is the collider):
  - "New here? Someone else owns this name — pick another." → button **Pick another name** → Launch, field selected.
  - "Is it yours? Approve this device from one you're already signed in on." → button **It's mine — show pairing words** → JoinerWords.
- No ceremony starts (no bind request posted, no beacon) until "It's mine" is pressed — today the request posts immediately, which is what makes a collision look like pairing AND spams the owner's registry with stranger bind-requests.

**Owner-side visibility** (the fun half): the worker already writes a `bind_attempt` inbox alert on every one-owner rejection, and the bindreq registry shows attempts. Add (ticketed, not now): a Panel→Fleet counter ("N attempts on your name") + a notifications toggle for collision attempts.

## D2 — one handle per device (the double-attest hole)

**Defect**: one device attested two handles back-to-back; both genesis publishes succeeded. The device now carries two identities (two vault key-spaces in one store), violating the one-owner-per-device model that sybil-resistance and the pairing trust story assume.

**Ground truth**: device keys derive from the machine fingerprint only (NOT handle-salted) — both attests presented the SAME device pubkey. The worker HAS the one-owner-per-device gate (`device_owned` reject + ownership index, fgtw-bootstrap) and the fgtw crate's tests prove it… against a current worker. The live fgtw.org worker predates the check or the index had no claim for the device — **verify + redeploy the worker** (deploy is standing-authorized) and the second genesis gets rejected at publish.

**But the worker gate alone is bad UX**: it fires AFTER the permanence interstitial, as a wall of error text. The fix is layered:
1. **Client early gate — the device-binding marker**: on first successful attest/join, write a small kete entry at a WELL-KNOWN address (not handle-derived — `vault_key("device_binding", device_pubkey)`), sealed under the device key, holding a keyed blake3 digest of the bound identity's MEMORY-HARD handle_proof (v1, 2026-08-23 — NOT the cheap party id: a pid marker was a fast offline handle oracle, and handles are SSN-grade). The probe worker consults it right AFTER the ~1s proof, before any network fetch: a mismatch shows the DEVICE BUSY line — "This device already carries an identity — resume it, or wipe it first (Panel → Security)." The wrong-handle cost is the same ~1s the right handle already pays to resume; no network either way. Legacy pid markers compare once for continuity and upgrade to v1 at the next successful attest.
2. **Worker gate** (redeployed): the backstop for a scrubbed marker.
3. Marker lifecycle: written on attest-success/join-success; deleted by Shred / Remove & shred / clean_device_for_reuse. A takeover-cleared session does NOT clear the marker (the device is still bound; only wiping unbinds).

**Cleanup for the already-doubled device**: shred from within the second identity (marker then re-binds to the first on next resume), or nuke_all. The worker index, once redeployed, holds the device for whichever identity's chain listed it first.

## D3 — the last device (LastRites)

> **SUPERSEDED 2026-07-17 — IDENTITY NEVER DIES.** The ruling below is reversed: there is no terminal op at all. The worker REFUSES any fold reaching zero members ("an identity must live somewhere"); the client refuses last-device sign-out; LastRites (the red flood, the lr-token, the pending-window drafts) is cut from code and design. Rationale: any destruction op — however ceremonial — is an annihilation surface for whoever holds a device + the handle (the daughter case); deleting the op deletes the attack class. Wanting out is served by RETIREMENT, not death: change the handle to something obscure (future handle-change work) and park the identity on one device. Hardware leaves a fleet in TWO signatures: the device signs its own departure (membership ends, sovereignty intact), and a SURVIVING member signs `device_release` to free the hardware brand — brands now SURVIVE departure, so a thief pressing sign-out on stolen goods walks away with a branded husk. The genesis-hash pin (below) stays load-bearing — for custodian supersession and defensive rendering — but "free-on-zero" is gone because zero is gone. The section is kept as testimony of the considered-and-rejected design. — orphaning the identity: chain folds to zero, no sponsor exists, `ensure_member` can never pass again, the name is dead, and every friendship dies with it. The user did exactly this by accident-adjacent curiosity.

**Should we allow it? YES — with ceremony.** Sovereignty doctrine is explicit: the subject signs their own exit, no exceptions, and total loss without custodians IS total. Blocking the last exit would make the fleet chain the user's jailer. But it must be impossible to do it *casually*:

- Detect `fold().len() == 1 && members[0] == us` at Remove & shred (and at plain Depart).
- Route thru **LAST RITES** — the red flood interstitial: whole screen red, red orb, and the truth in big letters, in the ruling's own register: "This is this identity's LAST device. Removing it ends the identity FOREVER — every conversation, every friendship, everything, gone. The name goes free for anyone to claim. There is no recovery. Not custodians, not new hardware, not you." Interstitial rules: only the re-labelled button proceeds ("End it — forever"), anything else cancels.

**THE RULING (2026-07-17, supersedes both earlier drafts — burn-by-default AND hold-the-tombstone):** the identity LIVES IN the fleet. Bind a device and the identity rides with it; devices are the only key. **Member count zero = the identity is GONE — not recoverable, by anyone, ever — and the handle is FREE.**

Why this is the clean design — zero is only REACHABLE deliberately, so the two recovery worlds split perfectly:
- **Members > 0, devices lost/stolen**: nobody signed an exit; the chain still lists the corpse fleet. THIS is what custodian supersession exists for — sign new devices in over the lost ones. (docs/total-loss; its scope is now explicitly members>0 only.)
- **Members = 0**: only the owner can produce this (departure is self-signed, no exceptions) — it is cryptographic proof of a deliberate end. Custodians are powerless BY DESIGN; the name returns to the pool. No dormancy heuristics, no held-by-corpse ambiguity for deliberate exits — the fold count IS the semantics.

Grief economics: burning buys nothing (an ended name frees immediately); denial-of-name collapses to SQUATTING (attest and hold), the baseline already priced by the memory-hard proof and worker-rate-limitable.

**The one load-bearing nuance — FREE must not mean INHERITABLE.** The identity seed derives FROM the handle string: a re-claimant of a freed name derives the SAME seed / party id / pubkey, so friends' pinned ids would MATCH the impostor and a fresh CLUTCH would render as "your friend re-keyed". The gate (REVISED 2026-07-17 — no epochs, no counters): **the generation id IS the genesis op hash.** Every chain's genesis op is signed and hash-linked, containing its eagle time + founding device key — a 256-bit unique no successor can reproduce, minted for free at claim. Friends **pin the genesis hash** at first-met; any fold refresh whose chain carries a different genesis renders that contact as a STRANGER — fresh contact, zero trust inheritance, the old conversation frozen as archive. The discriminator lives in the FRIENDS' pins (which exist because the friendship did); the handle itself carries NOTHING — no counter to store, no tombstone to keep, nothing that outlives the identity. Defense in depth: friends hold blinds of the old S, which no successor can produce (the re-key alarm surface). **Ship free-on-zero and the genesis-hash pin as ONE unit** — free without the pin gate is the inheritance bug shipped on purpose.

**Zero-fold = TOTAL PURGE (the "all traces of me GONE" rule).** Because nothing ever needs to reference a dead handle again, the worker stores no tombstone: the final departure folds to zero and the worker DELETES every hp-keyed trace — fleet chain, fan-out wraps, fstate blob, inbox, peer records, bindreqs — and releases the device-owner claims. The slot returns to virgin; a re-claim is the EXISTING first-come genesis branch, no new accept logic. Two stores the worker cannot find on its own ride LastRites as a CLIENT-side sweep before the departure publishes: the avatar blob (unlinkable by design — the departing device holds the pin and deletes it itself) and submitted diagnostic logs (retrieval tag derives from the seed).

**Friend-side rendering of a vanished chain**: `not_found` where members previously folded renders the contact as ENDED but FREEZES local state — verify-or-withhold: a lying/flaky worker must not be able to fake a death into destroyed archives. Only a NEW chain with a DIFFERENT genesis hash confirms succession (→ stranger). The friend's archive of the ended identity is the friend's — ostracism-not-erasure, pointing the other way.

Custodian supersession (members>0 recovery, docs/total-loss) needs its own continuity story under this model — a custodian-authorized replacement chain has a NEW genesis, so friends must be taught to accept it (quorum attestation in the new genesis vs. the pinned old hash); design deferred to the recovery work.

> **SUPERSEDED 2026-11-08 by P8** (the owner's tag is a FIDO2 credential, not a flashed seed: a seed written to a tag is a copyable bearer key, which the papers model exists to end). Kept as testimony.

**The backup tag — self-custody for a dollar.** A fleet member is a KEYPAIR in the chain, not a computer: an NFC tag flashed with a device-key seed IS a device, and it counts toward members > 0. Worried about losing your only phone? Add a tag and put it in a drawer.
- Enrol (from any signed-in device, Panel → Fleet → "Add a backup tag"): the phone mints a fresh device keypair, signs BOTH halves of the bilateral add with it (it holds the seed at birth — no protocol change, the tag is an ordinary member whose consent was signed at mint), binds it into the chain, writes the seed to the tag, zeroizes its local copy.
- Recover (new phone): tap the tag → load the device key → the probe reads `Member` → resume, no ceremony. Then bind the new phone properly as itself and either re-flash the tag with a fresh key or leave it.
- It is a BEARER instrument and the UI says so plainly: whoever holds the tag holds a device of your identity — guard it like a key, because it is one. No passphrase wrap (passless is passless; physical possession is the security model, same as the phone itself).
- Android-first (NFC read/write is native there); desktop needs a reader, later.

**The redundancy ladder** (what the onboarding teaches, in order):
1. Worried about loss? Add devices — a second phone, a laptop, a $1 tag in a drawer. Redundancy IS the recovery story.
2. Lost some devices? Any remaining member sponsors replacements.
3. Lost ALL of them, tag included? Custodian supersession over the corpse fleet (members > 0, nobody signed out).
4. Deliberately walked the count to zero? That was the exit. Gone forever, name free.

## Managing a contact: relationship, history, reach (2026-09-27)

Three independent choices; every "nuke", "unfriend", "just the waves" is a combination.

- **Relationship** — Friend · Appear offline · Mute · Ostracize (Boot) · Unfriend (a signed goodbye).
- **History** — what: all, waves only, attachments only, chosen rows; when: all time, before a date, a range.
- **Reach** — this device · my fleet · also ask them (a signed request their device may honour or ignore — never forced, device sovereignty).

Deletion is **discard**, never shred: a row is marked deleted (hidden, true-wins across the fleet) and its space is marked free; a distinct vault object (a recording, an attachment) is deleted outright. Until we own the flash, physical erasure is not ours to promise, and everything is encrypted at rest anyway.
Under braid v2 a row's text itself can be discarded once its 32-byte strand value is kept (docs/braid.md §6.2a); that step is not built yet.

**Built (v106):**
- **Boot sticks.** The booted ledger (`storage::booted`, per device, loaded with the contacts at attest) records every boot, ours or a sibling's tombstone. Every roster push re-emits it as a tombstone; the roster merge, the cloud-backup merge (whose restored stubs now stamp older than any roster entry) and chain-sync adoption all refuse a booted contact; a boot rewrites the cloud backup at once. A deliberate re-add, stamped after the boot, lifts it.
- **Clear waves / clear history** (the person page of your conversation with them, or a group's Manage, two-tap): every matching row is marked deleted across our fleet in page-sized pushes, and recordings and attachments are deleted from the vault on every device that applies the tombstones. The friend is not told; their copy is theirs.

**Not built:** Appear offline, Mute, Unfriend's goodbye, date-range and per-kind selectors beyond waves, "ask them too", and discarding a deleted row's text under braid v2.

## Build order (2026-11-08)

The 2026-07-17 punch list is done or superseded (the worker's one-owner gate, the device-binding marker and KnownHandle are built; LastRites was cut by D3).

1. **P3, contention shuts the network path.** Works on today's keys, so the deterrent comes first.
2. **Papers on the desktop:** the TPM HMAC over `machine-id` / `MachineGuid` in tohu, and the move (root_conversion generalised off Android). Then the Secure Enclave on macOS.
3. **Papers on Android:** StrongBox in the holder app, and the move.
4. **P2 and P4:** no papers means proximity only; with papers the words leave the network path.
5. **P5 and P6:** the same plate retires its ghost; the owner releases a plate (chain op + worker).
6. **P8, the tag:** FIDO2 `hmac-secret` enrolment and sponsoring.
7. **One identity, every app:** the shared attest and the Unbound / Owned screens (one-identity.md's order).
