# Headless lifeline — the bridge must not die with X

## The incident that justifies this (2026-09-04 → 09-07)

Xorg aborted mid-display-hotplug while rustdesk injected a remote cursor move (`ProcXTestFakeInput → drmmode_sprite_move_cursor` in the amdgpu DDX — a use-after-invalidate against a vanishing CRTC).
The kernel, network, and user manager stayed alive for three days; photon died with the X session because it is an X client, and with it died the bridge — the exact tool that would have diagnosed and rebooted the box remotely.
The machine sat at a black greeter until a human drove out and pressed reset.
Rule extracted: **the remote lifeline may depend on the network and the vault, never on a display server.**

## Shape

Photon stays ONE binary. A new `--lifeline` mode runs the network core with no fluor, no winit, no display:

- Open the device vault, auto-attest thru the reboot capsule (tohu session capsule — lifeline mode is exactly as available as unattended mode, which this box arms; no capsule → lifeline idles pre-attest and still serves nothing sensitive).
- Spin the normal network stack and pump it in a plain loop: `advance_protocol`, `check_status_updates`, the bridge drains — the same `PhotonApp` methods, driven by a headless tick instead of the winit loop.
- Host the bridge exactly as today: commands in, delta streams out, Stop honored.
- No UI, no textboxes, no canvas: `PhotonApp::new()` already leaves every widget `None`, and draw-adjacent paths no-op on `as_mut()` misses; the audit below names the exceptions.

**Reattach is a process swap, not an in-process miracle.** When a real session launches photon normally, the single-instance control channel already handles the encounter: the lifeline instance recognises a full-UI sibling asking for the lock, yields it, and exits; the full instance auto-attests thru the same capsule. No X-reconnect machinery in winit, ever.

## Phases

**A — the headless pump.**
`main.rs --lifeline` branch: skip `run_app`, construct `PhotonApp`, call a `lifeline_init()` (the network/vault/attest subset of `FluorApp::init` — no widgets, no `Context`), then loop: pump ticks on the WAKE edges (a headless `WakeSender` impl over a condvar) with the same cadences the UI loop provides.
The audit: every `ctx.text`/widget touch reachable from `advance_protocol`/`check_status_updates`/bridge drains must no-op headless (most already do; `load_you_fields`-class calls are page-gated and unreachable).

**B — supervision.**
A user-manager systemd unit (`photon-lifeline.service`, `Restart=always`, `WantedBy=default.target` — the user manager survived the whole incident) that starts `photon --lifeline` ONLY when no instance holds the single-instance lock.
Full photon running → unit idles; X session dies taking photon with it → unit relaunches the lifeline within seconds; user logs back in and opens photon → lifeline yields.
Must not fight dev.sh's reload (which kills + relaunches deliberately): the lock check makes the unit a fallback, never a competitor.

**C — remote hands.**
With the lifeline holding the bridge, the operator can `sudo reboot` a wedged session away (auto-attest + autostart bring the full app back), fetch logs, or redeploy — the incident's whole wishlist.

## Non-goals

- In-process X reconnect (winit's x11 backend dies with the connection; fighting that is complexity with no payoff over the process swap).
- Lifeline on Android/Windows (Android has FCM+JobScheduler wake paths; Windows sessions don't evaporate the same way — revisit if the field says otherwise).
- Any UI in lifeline mode. It is a bridge host and a network presence, full stop.
