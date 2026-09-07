# Key custody — who vouches for a waking device

Design sketch 2026-09-04 (Nick, riffing off docs/device-lease.md): the lease's delegated session isn't a guest-only trick — it's the general shape. The identity seed and decryption roots should be AT REST NOWHERE; day to day, the FLEET delivers session material to a waking device, and the typed handle becomes the root of last resort instead of the daily ritual.

## The voucher spectrum

Every wake answers one question — who vouches for this device right now?

| voucher | lifetime | what it proves | cost to the human |
|---|---|---|---|
| **wairua** (per-boot secret) | until power interruption | same boot, same session — nothing left the rail | zero (seamless resume) |
| **fleet** (an owned device approves) | until recall/lockout | possession of live hardware AND knowledge: the handle is typed ON THE APPROVER | handle entry, on hardware you chose |
| **handle** (typed) | the root itself | the human knows the name that IS the key (seed = BLAKE3(handle)) | typing a secret on THIS hardware |

**The knowledge factor is never traded away (Nick 2026-09-04, revising the first draft).** A tap-only fleet vouch would make possession sufficient: a thief with the whole backpack — rebooted laptop plus still-attested watch — would authenticate by powering on and tapping approve. Rejected. The fleet rung moves WHERE the handle is typed (onto an approver you chose), never WHETHER: waking hardware + typed handle on live owned hardware = the same two factors as today, with the secret kept off the least defensible keyboard in the room. Matches the shipped precedent: the unattended toggle already demands handle re-entry to arm AND disarm.

Why WHERE matters: ferros and Android can give the handle entry a kernel-protected input path (keylog-resistant by construction); Windows and macOS cannot promise one. The ladder's payoff is typing the handle only on platforms that can defend the keyboard — and NEVER on borrowed hardware.

Falling down the ladder: boot vouches until reboot; the fleet vouches when an owned device is live AND the human proves the handle there; the handle lands on the waking device itself only at the root — first device, total loss, all-offline.

## Today (shipped + spec'd)

```mermaid
flowchart TD
    W[device wakes] --> C{session capsule\nopens under wairua?}
    C -- "same boot" --> R[resume — roots from the capsule\nseed at rest: nowhere]
    C -- "reboot: wairua died" --> U{unattended toggle\narmed? — off by default}
    U -- yes --> RC[device-bound reboot capsule\nauto-attest]
    U -- no --> H[human types the HANDLE\non this hardware]
    H --> D["memory-hard derive:\nidentity_seed · vault_seed · handle_proof\n(tohu session registers, RAM only)"]
    D --> V[vault opens · fleet announces]
```

The invariant already held everywhere: nothing durable stores the seed; the capsule stores roots only under a key that dies with the power rail. The cost: every reboot spends a handle entry ON the waking device — the one place a keylogger would sit.

## Target (the ladder completed)

```mermaid
flowchart TD
    W[device wakes] --> C{capsule opens\nunder wairua?}
    C -- "same boot" --> R[resume]
    C -- reboot --> F{any OWNED device\nreachable?}
    F -- yes --> A["fleet inbox: wake request\n→ HANDLE typed on the approver\n(kernel-protected input where the platform has it;\nthe waking device never sees it)"]
    A --> S["fleet DELIVERS session material:\nrouting capability + wrapped vault root\nsession-scoped · killable by recall"]
    S --> V[vault opens · fleet announces\nseed still at rest NOWHERE]
    F -- "no — fleet dark" --> H[handle typed: the root of last resort]
    H --> D[derive registers] --> V
    G[guest on BORROWED hardware\ndocs/device-lease.md stage 2] --> A
```

The guest session and the owned-device wake become the SAME approval flow — the lease's stage-2 machinery, pointed at your own hardware. Lockout unifies too: a locked-out device is exactly one the fleet refuses to vouch for; recall of a delivered session is the same verb for guests and for your own drawer phone.

## Invariants (non-negotiable)

1. **The identity seed itself never crosses a wire and never rests.** What the fleet delivers is session material: routing capability, streamed history, and a WRAPPED vault root — killable, session-scoped, useless off-device. (The wrapping design rides the fleet-key redesign's ira-wrap work — docs/fleet-key.md.)
2. **The handle lands on the WAKING device only at the root of the ladder** — first device, total loss, fleet dark. At the fleet rung it is typed on the APPROVER instead. The ladder exists to keep the handle off indefensible keyboards — the just-rebooted, the borrowed — never to remove the knowledge factor.
3. **No timers.** Wairua dies at the power rail; fleet vouching dies at the recall/lockout edge; nothing expires by clock.
4. **Approval is an owned-device edge WITH the knowledge factor** — the fleet-inbox bind-attempt alert carries a handle-entry gate on the approver, the unattended arm/disarm precedent generalized. Possession alone authenticates nothing, ever.

## Open questions

- Exactly what the wrapped vault root is: the ira-wrap from the fleet-key redesign is the natural candidate, but that spec is still under Nick's review — this doc must not front-run it.
- The all-devices-rebooted-simultaneously fleet: everyone's wairua died, nobody can vouch — one device takes the handle entry and re-seeds the ladder. Fine, but the UX should say WHY ("your other devices are dark").
- Whether the unattended reboot-capsule toggle survives this design or is subsumed by it (the capsule keeps the single-device unattended case; it remains the ONE deliberate possession-only surface, off by default, armed by handle entry).
- How light the approver-side entry can get on kernel-protected platforms (full handle vs a shortened proof) — undecided; the full handle is the conservative default.
