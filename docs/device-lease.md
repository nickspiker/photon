# Device lease — borrowed hardware without moving title

Design 2026-09-04 (Nick + review of the retired/Release support call). Extends docs/lifecycle.md; supersedes the parked "transfer vs loan-annotation" question in the loaner notes: the answer is a LEASE, and transfer (Release) stays the rare deliberate act it already is.

## The axes

Every device state is a point on three independent axes — the verbs stop being confusing the moment they're named:

| axis | meaning | verbs |
|---|---|---|
| membership | is it a member of an identity's fleet? | bilateral departure (leaver signs + survivor countersigns) |
| access | do keys/routing currently serve it? | lock out / unlock — the routing layer |
| title (brand) | whose identity may the HARDWARE attest into? | Release (title transfer), **lease** (tenancy, new) |

Existing states: active (✓✓ yours), locked-out/drawer (✓ ✗ yours), retired husk (✗ ✗ yours), released (✗ ✗ freed). The lease adds: **title yours, someone else's identity resident.**

## The lease

- **Grant**: the owner's fleet signs an annotation on the brand naming a guest identity (or open). Worker attest rule becomes: `identity == brand owner OR a live grant names it`. Title never moves.
- The guest's device joins the GUEST's fleet as an ordinary member; their vault seals under their handle on the owner's disk — unreadable to the owner, symmetrical privacy.
- **Recall**: an owner-signed edge enforced at the ROUTING layer (the lockout machinery pointed at a guest). No timer, no expiry — the recall is the event. Recall protects title and data, not physical repossession.
- **Sovereignty**: recall never evicts the guest from their own fleet (nobody signs away someone else's membership). Their fleet sees the device go dark; they bilaterally depart it at leisure; their claims stay theirs (de-attest keeps claims, dormant).
- **Vault hygiene**: when the brand OWNER re-attests on recalled hardware, foreign app-private vaults are wiped. Clear edge, no timer; the guest lost nothing (FLEET-HOLDS-HISTORY).

## The airport (stage 2 — the point of all this)

Borrow a stranger's device; tap approve on your watch; your conversations appear.

**The handle NEVER touches the borrowed device** — typing it there would disclose the seed (seed = BLAKE3(handle), a keylogger away from identity theft). Instead:

1. The borrowed device (its owner taps "lend") mints a **session request** — a fresh keypair + display words, the pairing-v2 binding-request shape.
2. The request reaches the guest's fleet as a bind-attempt alert — the fleet inbox's designed v1 message.
3. An OWNED device (watch/phone) shows it; the human approves; the fleet delegates a **session key**: routing capability + streamed history. The identity seed exists nowhere on the borrowed hardware, ever.
4. The fleet streams history to the session (fleet-holds-history makes the fleet the source; the borrowed device is a disposable viewport).
5. Walk-away = recall from any owned device, or the hardware owner's own recall. The session key stops routing; the sealed session residue opens for nobody.

## Staging (REVISED 2026-09-04 — the typed-handle stage is DEAD)

A "household lease" where the guest types their own handle on the owner's hardware was staged first and CUT the same day (Nick: typing a handle on a loaned device "makes me nervous" — correctly). A handle is seed-grade; entering it on foreign hardware hands that hardware's owner the guest's identity. Family trust changes who received the seed, not whether it was given. docs/key-custody.md's invariant — the borrowed keyboard never sees the handle — is universal, so there is exactly ONE lease flow:

- **The delegated guest session**: grant + recall at the worker, session request from the loaned device, approval + handle entry on the GUEST'S OWN hardware, killable session key delivered. Builds with the pairing-v2 + fleet-inbox + session-capsule cluster, after voice calls.
- Until it exists, the departure-intent menu ships with TWO options (new owner / desk) — a loaner option that could not complete would be the dangling-flow sin this rework exists to kill.

## Interactions with standing doctrine

- Identity never dies / no terminal op: unchanged — a lease creates nothing destructible.
- Bilateral departure: unchanged; a leased device departs the guest's fleet the normal way.
- Brand anti-theft: unchanged — a thief gets no lease grant; the husk rule holds.
- Release-as-a-checkbox on Approve: rejected; Release-as-an-INTENT: adopted (2026-09-04, the retire/Release incident). Approving a departure asks what's happening — new owner (countersign + release, complete at the tap), desk/wiped (countersign, brand kept, retired row = inventory not a to-do), loaner (joins the menu only when the delegated session can complete it). Lockout stays off this menu — the still-a-member drawer case has its own verb. Every option completes or visibly holds with a named reason; no flow mints a second step discovered later. Plus joiner-side honesty: branded hardware TELLS its would-be new user that the previous owner can release it.
