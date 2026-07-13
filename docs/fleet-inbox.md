# Fleet inbox — alerts, notices, and the broadcast channel

One mechanism for getting a message to a whole fleet: worker-observed security events, key-signed product notices, and member-to-fleet broadcasts.
Status: DESIGN (2026-07-12). Nothing built; v1 slice is the bind-attempt alert.

Companions: [device-lifecycle.md](device-lifecycle.md) (the flows that emit events), [rekey-threat-model.md](rekey-threat-model.md) (the re-key notification rides this), [updates.md](updates.md) (the update-available push answers its open poll-cadence question).

## Why

Three consumers already queued, all needing the same carrier:

1. **Bind-attempt alert.** Someone tries to enrol a device claimed by your identity (`device_owned` rejection, or any pairing request posted for your handle) — every device in your fleet should see it immediately. This is the theft tripwire: a stolen device being enrolled elsewhere becomes visible the moment it's tried, not never.
2. **Re-key notification.** The rekey-threat-model work needs "your peer's chain re-keyed, eyes open" delivered fleet-wide.
3. **Update-available push.** A release lands → a signed notice → live clients run the stamp-window check now instead of on the next poll. Product updates stop being a polling problem.

Plus the loaner-recall announcement once loaners exist (recall lands in the borrower fleet's inbox instead of being discovered at the next refused announce).

## Mechanism

`inbox/<handle_proof>/<ts>.vsf` on R2 — [relay's](network.md) store-and-forward mechanics, but addressed to the *identity* instead of a device.
Any fleet member drains it; every member sees the same stream; timestamped keys give ordering and TTL cleanup for free.

Wake-up rides what already runs:
- **PeerUpdateHub** WebSocket broadcast for live clients (the DO's `/broadcast` endpoint, same as announce uses today).
- **FCM** `peer_update`-style wake for backgrounded Android.
- **Attest/resume drain** as the floor — a device that was off drains the backlog on next launch.

## Authorship classes

Trust differs by author, so the class is structural, never inferred from content.
The worker stamps the class server-side at write time: events are written only by the worker's own handlers, member notices only thru a signed `inbox_put` whose author signature the worker verifies against the CURRENT fleet chain fold, release notices only against the release key.
A member cannot write anything that renders as a worker event.

1. **Worker-observed events** — routing-layer facts only the worker sees: "device X attempted to bind to your fleet at T, refused (`device_owned`)", "a pairing request was posted for your handle".
Advisory, display-only, no secrets.
The client renders them as network notices, never acts on them automatically.
2. **Release-key notices** — product updates and admin broadcasts, signed by the release key and verified client-side exactly like the binary manifest; the worker stays untrusted for content.
Global channel, not per-fleet: one well-known notice feed every client checks alongside its own inbox.
3. **Member notices** — a device telling its siblings something, signed by the member device key and sealed under the fleet key (the worker can't read them — consistent with the encryption wall).
Rendered with the authoring device's name (the canonical `device_name_default` derivation), never as anonymous "your fleet".

## Threat model: the stolen member

A stolen device that is STILL a fleet member holds a valid device key.
Until it is removed and the fleet key rotated, it IS the fleet as far as authorship goes: it can author member notices (impersonating "one of your devices"), and it reads fleet-key-sealed notices and every alert.
The inbox does not solve stolen-device — it inherits the fleet-membership trust boundary.
What it changes is VISIBILITY: theft becomes observable (bind attempts elsewhere trip alerts on every remaining device) instead of silent.

Design consequences, so a rogue member's reach stays bounded:

- **Attribution, always.** Every member notice displays WHICH device authored it. A rogue member can speak, but never anonymously — the owner sees "kitchen tablet says…" and knows the kitchen tablet is in a stranger's hands.
- **The inbox is never a control channel.** No message class can steer a sibling into an action with authority — no "wipe yourself", no "trust this key", no config changes.
Anything with authority stays on the existing signed-op paths (fleet chain ops, CLUTCH, the update manifest's own signature); inbox text is eyes-only.
This is load-bearing: without it, one stolen device converts into command of the whole fleet.
- **There is no chain eviction — membership is consent-only (decided 2026-07-12: removal requires the device's OWN signature, no exceptions).** A hostile member is contained above the fleet instead: session decay, vault handle-gating, and S/friendship-layer re-key; the fleet key is accordingly demoted to low-blast-radius state only (the roster), because every ever-added device can recover every epoch until identity re-key.
Member notices from a stolen device therefore remain possible indefinitely — which is why attribution and the no-control-channel rule above are absolute, and why the inbox's theft value is the alert (visibility), not any enforcement.
- **Worker events are member-forgery-proof** by the write-path separation above; the residual trust is in the worker itself, which is already trusted for routing (and nothing more).

## Anti-spam

Inherited, not invented: worker events are rate-limited facts about your own fleet; member notices require a valid current-member signature; release notices require the release key.
Nobody else can write to your inbox at all.
Per-inbox retention cap + TTL mirror relay's.

## v1 slice

The bind-attempt alert end-to-end: worker drops an event on `device_owned` rejection and on `pair_put` for your handle → inbox drain on resume + hub wake → rendered as an event-shown, interaction-cleared notice (no timers, per the no-time-based-UI rule).
Proves the mechanism with the smallest consumer; re-key notification and update push follow on the same rails.

## Open questions

- Retention: cap per inbox (count and bytes) and TTL — relay's numbers, or tighter since alerts age fast?
- Does the global release-notice feed use the release key itself or a dedicated notice key (release key stays cold except for builds)?
- Loaner-recall events wait on the loaner design landing (dormant claims, transfer-vs-annotation — see the device-lifecycle discussion).
