# The identity/device lifecycle — flow tree, screen names, conventions

> Status: DESIGN, agreed 2026-07-17 (session notes). This is the canonical map of every screen a device passes thru from first launch to final exit, the names we call them, and the visual conventions they share.
> Driven by three live flow defects: the handle-collision screen reading as device-pairing, a device double-attesting two handles, and the last device of a fleet shredding itself into an orphaned identity with no ceremony.

## The state model (what the screens render)

Three independent axes; every screen is a view of one combination.

**Device** (this physical machine):
- `virgin` — no identity material on the device.
- `bound(I)` — carries identity I's vault + is (or was) listed in I's fleet chain. THE RULE: **one identity per device, one-or-more devices per identity** — a device is never bound to two identities at once.
- A wiped device returns to `virgin` (its chain listing, if never departed, lingers as a ghost the worker's one-owner index still holds — see D2).

**Identity** (the handle's fleet chain, global):
- `unclaimed` — no chain at this handle_proof.
- `live` — chain folds to ≥1 member device.
- `abandoned` — chain exists, folds to 0 members (every device departed). Nobody can enrol (no sponsor exists); the name is dead. See D3.
- `superseded` — custodian-quorum recovery replaced the chain (designed, not built — docs/total-loss).

**Session** (RAM, per boot):
- `signed-in` — tohu registers live; app resumes to Ready without ceremony.
- `signed-out` — reboot/de-attest cleared the registers; the handle must be re-typed (Member-resume, ~1s proof, no new claim).

## Conventions

- **Flood states**: a whole-screen colour + matching orb tint marks an exceptional device state that must be recognizable across the room. Amber = development build (exists). **Green = "Selected!"** — the joining device is being confirmed by its sponsor (designed, pending build). **Red = final-exit interstitial** — the screen that ends an identity (proposed here, D3). Floods are themes, not dialogs: the whole surface changes.
- **The interstitial pattern** (from the permanence warning, `LaunchState::Confirm`): an irreversible act arms a full explanation + a re-labelled button; ONLY pressing that button again proceeds; every other interaction — editing, tapping anywhere else, navigating — cancels. No timed anything (event-shown, interaction-cleared, per the no-time-based-UI rule).
- **Notices**: event-shown, interaction-cleared bands/toasts. Green band = confirmations ("Device added √"), amber = warnings (clock-off, degraded vault), never timed.
- **Screen names** (canonical, use these in code comments + tickets): `Launch`, `Claim` (the permanence interstitial), `Attesting`, `KnownHandle` (see D1), `JoinerWords`, `JoinerSelected` (green flood), `SponsorAdd` (the words-entry screen, today `AppState::AddDevice`), `SponsorConfirm` ("did it turn green?"), `Ready`, `Conversation`, `Panel(page)` (the settings panel), `LastRites` (red flood, D3).

## The flow tree

```
LAUNCH  (handle box; the only entry screen)
│
│  [device already bound(I≠typed)?  → DEVICE BUSY line — see D2 — offer: resume I / Panel→Security to wipe]
│
└─ Attest pressed → PROBE (silent: ~1s proof, chain fetch + fold)
   ├─ Fresh     → CLAIM interstitial ("Yes — forever")
   │               └─ second press → ATTESTING → READY          (genesis; worker one-owner gate applies, D2)
   ├─ Member    → ATTESTING (resume; no ceremony) → READY
   ├─ JoinOurs  → KNOWN HANDLE                                   (the collision-ambiguous branch — D1)
   │               ├─ "It's mine — approve from my other device"
   │               │     → JOINER WORDS (this device shows words; ceremony polls)
   │               │        └─ sponsor binds → JOINER SELECTED (green flood, "Selected!")
   │               │             └─ sponsor confirms → signed in → READY
   │               └─ "Pick another name" → LAUNCH
   └─ Taken     → error line → LAUNCH                            (genesis/identity mismatch — near-theoretical)

SPONSOR side (existing device):
READY → orb → PANEL(Fleet) → SPONSOR ADD (types the joiner's words)
   └─ match → bind published → SPONSOR CONFIRM ("is it green and says Selected?")
        └─ yes → fleet-key rotation (the joiner's signal to proceed) → notice band "Device added √"

EXITS (PANEL → Security, the destructiveness ramp — green→yellow→orange→red):
   Lock            — reversible; re-type the handle to resume.
   Depart          — self-signed fleet departure; device keeps nothing? (keeps NOTHING identity-flavoured — departure then wipe of identity material; device → virgin, identity lives on the other devices)
   Shred           — crypto-wipe WITHOUT departing (the chain still lists this device; the worker one-owner index keeps the ghost until a sibling removes it — the escape hatch for a dead device is removal-by-… nothing today; ticket)
   Remove & shred  — depart THEN wipe, gated on the departure landing.
        └─ if this is the fleet's LAST member → LAST RITES (red flood interstitial, D3) before anything happens.

SESSION:
   reboot → signed-out → LAUNCH → Member → resume.
   takeover detected mid-app (AlreadyAttested: fold-verified foreign chain) → session cleared → LAUNCH.

RECOVERY (future): custodian-quorum supersession (docs/total-loss) enters at LAUNCH as its own probe branch.
```

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
1. **Client early gate — the device-binding marker**: on first successful attest/join, write a small kete entry at a WELL-KNOWN address (not handle-derived — `vault_key("device_binding", device_pubkey)`), sealed under the device key, holding the bound identity's handle_proof + party id. The PROBE consults it first: typed handle resolves to a different identity than the marker → the Launch screen shows the DEVICE BUSY line: "This device already carries an identity — resume it, or wipe it first (Panel → Security)." No proof spent, no network.
2. **Worker gate** (redeployed): the backstop for a scrubbed marker.
3. Marker lifecycle: written on attest-success/join-success; deleted by Shred / Remove & shred / clean_device_for_reuse. A takeover-cleared session does NOT clear the marker (the device is still bound; only wiping unbinds).

**Cleanup for the already-doubled device**: shred from within the second identity (marker then re-binds to the first on next resume), or nuke_all. The worker index, once redeployed, holds the device for whichever identity's chain listed it first.

## D3 — the last device (LastRites)

**Defect**: Remove & shred on the fleet's final device departs + wipes with only the standard two-tap — orphaning the identity: chain folds to zero, no sponsor exists, `ensure_member` can never pass again, the name is dead, and every friendship dies with it. The user did exactly this by accident-adjacent curiosity.

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

## Implementation punch list (ordered)

1. Redeploy the fgtw.org worker (one-owner gate + index live). Verify with a two-handle genesis attempt against a scratch device key.
2. Device-binding marker + the Launch DEVICE BUSY line (D2 client gate).
3. KnownHandle screen (D1): copy + two affordances; move the bind-request post + beacon behind "It's mine".
4. LastRites (D3): last-member detection on both Depart and Remove & shred + the red flood interstitial (reuses the amber/green flood mechanism).
5. JoinerSelected green flood + sponsor-confirm hold (already designed, waiting — same flood mechanism as 4).
6. Ticket: collision counter + notification toggle on Panel→Fleet; name-release worker support.
