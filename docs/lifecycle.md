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
- Route thru **LAST RITES** — the red flood interstitial: whole screen red, red orb, and the truth in big letters: "This is this identity's LAST device. Removing it ends the identity forever — conversations, friendships, everything." Interstitial rules: only the re-labelled button proceeds ("End it — forever"), anything else cancels.

**The tombstone is an EPOCH BOUNDARY, not a burn** (decided 2026-07-17, superseding the burn-by-default above-session draft):
- Burn-forever is a griefing primitive: attest → depart → dead name at ~1s of proof each — permanent namespace destruction for free. So a deliberately-ended name must eventually RELEASE.
- But naive release is identity inheritance, NOT name recycling: the identity seed derives FROM the handle string, so a re-claimant derives the SAME seed / party id / pubkey — friends' pinned ids MATCH the impostor, their clients fold the new chain for the same handle_proof, and a fresh CLUTCH renders as "your friend re-keyed". (An earlier draft claimed pins protect here; they do not — same string, same keys.)
- Resolution: the zero-member fold IS the tombstone — signed, self-authorized testimony that the identity ENDED (departure is the one op only the owner signs). A dead name may be re-claimed, but the new genesis starts the next EPOCH, and the **pin-set carries the epoch**: a friend's client that sees its pinned contact's chain superseded past a tombstone renders the successor as a STRANGER — fresh contact, zero trust inheritance, the old conversation frozen as archive. No silent re-key-as-you.
- This UNIFIES with custodian recovery (docs/total-loss), which is already chain SUPERSESSION: deliberate release = self-authorized supersession to zero; total-loss recovery = custodian-quorum supersession. Same epoch mechanism, different authorizer — one concept, and the pin-set tracks both.
- Defense in depth: the blind-S machinery is an identity-continuity witness — friends hold blinds of the old S and a successor epoch cannot produce it; the in-flight re-key notification work is the alarm surface.
- Grief economics after: burning buys nothing (names recycle); denial-of-name collapses to SQUATTING (attest and hold) — the baseline already priced by the memory-hard proof, worker-rate-limitable if mass-squatting appears. The LOST fleet (devices dead, departure never signed) stays held-by-corpse forever — it must, or "release inactive names" becomes "claim anyone's dormant identity"; only custodian supersession unlocks it.
- Build order: LastRites ships first with today's held-tombstone behaviour (chain keeps the empty fold, name stays dead); epoch-in-pin-set + worker genesis-over-tombstone land WITH the re-key/supersession work — release semantics must not ship before the epoch gate that makes them safe.

## Implementation punch list (ordered)

1. Redeploy the fgtw.org worker (one-owner gate + index live). Verify with a two-handle genesis attempt against a scratch device key.
2. Device-binding marker + the Launch DEVICE BUSY line (D2 client gate).
3. KnownHandle screen (D1): copy + two affordances; move the bind-request post + beacon behind "It's mine".
4. LastRites (D3): last-member detection on both Depart and Remove & shred + the red flood interstitial (reuses the amber/green flood mechanism).
5. JoinerSelected green flood + sponsor-confirm hold (already designed, waiting — same flood mechanism as 4).
6. Ticket: collision counter + notification toggle on Panel→Fleet; name-release worker support.
