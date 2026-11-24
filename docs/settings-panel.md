# Settings / About / Help panel

Opened from the **orb** (top-left chrome badge).
The orb is the panel *entry*, never a direct action — device management and everything else are pages *inside* the panel.

Photon has no account and no server, so this panel is less "a bag of toggles" and more the **control surface for a self-sovereign identity**.
Three pages carry real weight (they are capabilities, not preferences): **Fleet**, **Security/Recovery**, and the log **Diagnostics**.
The rest is table stakes.

## Build order

Scaffold first: build every page and stub every control (checkbox, radio, dropdown, slider, toggle, button) with **no wiring** — the point is to see the layout and exercise the control set before any behaviour is attached.
Wire features one page at a time afterwards.

## Pages

### You
- Handle (public username) — display, copyable, voca-encoded so a double-click grabs the value.
- Avatar — current avatar + change (system image picker → the existing encode/upload chain).
- Pubkey / handle_proof — display, copyable.

### Fleet
The load-bearing page for multi-device with no server.
- List of bound devices, each with its **two-word name** (e.g. `zesty-otter`) + online / last-seen.
- **Add device** — the pairing-words flow.
- **Rename** — self-rename propagates to the fleet roster so siblings show the new name; renaming a sibling is allowed (you manage your own fleet).
- **Unbind this device** — see Security; a sibling is **revoked** there, or with Lock out on its row here.
- **Device sidecar**: each device keeps its own device-local preferences.
  Split: **device-local** = this device's name, notification volume, appearance, its log.
  **Fleet-wide (synced)** = identity, contacts, avatar, party colours.
  The device-local sidecar is a device-keyed store that must NOT propagate thru the synced vault.

### Security
One page on every TOKEN app: the rows, their order, their words, when each is enabled and why not, and the confirmation each needs are `fgtw::security` (owner, 2026-11-24: "I'd like to get it synchronized in the FGTW crate so ferros and Photon render the same"); Photon draws them (`ui/photon_app/security.rs`) and carries each act out with its own hooks.
Named by **destructiveness**, least to most, and by the owner's words for leaving (2026-11-24: "not retire, unbind/bind, shit like that. retire means never again which is wrong."):
- **Lock** — zeroize the in-RAM seed, keep the vault ciphertext on disk. The manual version of the reboot boundary: re-unlock with your handle. Tightening only, so no confirmation.
- **Revoke** / **reinstate**, one row per other device of the fleet — the remote act: a fleet-chain lock of a sibling that may be offline or already sold, so it can't be shredded, only barred from announcing, writing or opening anything sealed after (the worker's refusal survives a wipe; the fleet key rotates away from it). The "I sold it and forgot to wipe" safety net. **Reinstate** undoes it. Revoke, not "unbind remotely": an unbound device may be bound again by anyone, a revoked one is barred until its fleet reinstates it.
- **Shred** — crypto-shred of this device's copy: its keys destroyed (data is already ciphertext, so the dying key makes it unrecoverable), nothing deleted (below). The device **stays in the fleet**, so attesting again here with the same handle fetches everything back: the "wipe local storage, re-fetch from fleet" that format bumps (e.g. vsf v7→v9) make genuinely useful.
- **Unbind this device** — it leaves the fleet (the departure, `fgtw::flow::depart`) and its keys here are destroyed; anyone may bind it again. Never "Retire": retire means never again.
- Photon's own rows follow the shared ones: Kill, the Shell switch, the posture strip, Load on startup, the lifeline, unattended.

Every act that destroys or bars something of the person's needs the identity proved, never a tap a child could make (owner, 2026-11-24). In Photon a revoke or a reinstatement is confirmed on the attest screen (this device de-attests and the act runs inside the next attest whose handle is this identity's), a shred or an unbind by the handle typed under its row, its memory-hard proof matched.

**Nothing is destroyed without the person** (owner, 2026-11-24):
- "if it does not get my consent from my other devices or a correct handle entry, nothing gets wiped. Lock is fine, but no wipe and certainly no unbind."
- On shred: "if you cannot attest, there's ZERO reason my shit should get wiped and pretty sure we discussed, that's mostly key management formalities and proper lifecycle/design."
- On the bulk erase unbind used to do: "whoever's dumbfuck idea to add more writes to an SSD that has a fixed number of writes is beyond me but it's dead wrong. Sand on a beach my friend."

So:
- **No wipe before attest.** Before attest the page says nothing here can be changed or destroyed, and has no row. (The pre-attest Wipe of 2026-09-16 is gone.)
- **Shred and unbind destroy keys and delete nothing.** What is sealed becomes unreadable when its keys are wiped from memory and the vault salt is renewed: one small write, the security chip's where there is one (a TPM NV index; StrongBox or the Secure Enclave by a key it deletes), the vault's device scope where there is none (the owner's design, 2026-11-24; key = derive(addr ‖ vault seed ‖ device secret ‖ salt)). The store's own rotation reclaims the room when it needs it. Until the salt lands, the shred still discards the vault thru kete.
- **Unbind follows fgtw::security's `UNBIND`, in order:** the handle; the request to the fleet; a member's consent, waited for with no timer; the consented Remove, signed by the device and the approver; the fleet key turned over (Photon: on the fold-change edge where a surviving member sees the device gone, whichever member sees it first; the worker's epoch guard settles a race); the device that left forgets its keys.
- **A wrong handle, no consent, a deny, an unreachable fleet or a power cut changes nothing.** At most the app locks.

### Recovery
The only page about *getting back in* rather than *clearing out* — kept separate from the wipes.
- **Custodians (v1)**: a single opt-in checkbox — "be a custodian for others: yes/no". Choosing *your own* custodians is later.
- **Identity backup**: the reinstall/SAF backup (Android) — "reinstalling won't ask for your handle". Decline-friendly.

### Appearance
- Theme (light / dark chrome).
- Party colours — swap the placeholder to the perceptual L≈50 % set.
- Zoom / text size.
- Colour calibration (Android panel).

### Notifications
- Chime (chirp): global on/off + per-contact override.
- Presence visibility.

### Updates
Platform-conditional — one slot, two personalities (wire with `#[cfg]`):
- **Non-Android** (Linux / macOS / Windows): **"Install updates automatically" — on by default.** Linux self-replaces its own binary with no gate; macOS/Windows are promptless *per update* provided the build is signed/notarised (a one-time pipeline cost already paid in `deploy.sh`, not a per-update tap).
- **Android**: no silent install exists for a sideloaded APK, so the toggle becomes **"Notify of updates" — on by default.** The app auto-*downloads* in the background (no permission needed), notifies when an update is ready, and on the first **Install** tap prompts the `REQUEST_INSTALL_PACKAGES` "Install unknown apps" grant (asked at install-time for context, never at toggle-time) plus a one-time explainer that each install needs a confirm tap — the price of staying off the Play Store.

### Diagnostics (log)
The novel one — decentralized, handle-indexed, fleet-replicated, self-expiring crash reports, no server slurp.
- The on-device VSF log (16 MiB + jittered 24–48h) is only ever *mutated* by **Clear** (the Clear button is the sole writer; everything else reads).
- **Snapshot** reads the log and clamps the *snapshot* (not the log) to **8 MB / 24h**, plus an optional note.
  Two capture modes: the rolling window, or Clear → reproduce the bug → Submit for a tight focused trace.
- **Submit**: the snapshot replicates across your fleet and self-wipes at its eagle-time expiry (24h); available on-call from any fleet device (referenced by handle, not device).
  A provenance hash goes to the FGTW bootstrap keyed by handle, as a discovery + integrity anchor; the log itself stays on device and is P2P-pullable on demand, verified against the hash.
- **Access model (v1): handle-as-key.** Logs are name-scrubbed (no handle, no identity_seed in the content) and fetched/decrypted by handle — so a peer who knows your handle can read your scrubbed logs, which is accepted because the content is innocuous.
  The dev needs the user to message their handle first, then fetches by handle.
  Choosing an admin to seal logs to (so decryption isn't handle-only) is deferred to the **admin panel**, way down the road.

### About
The mental model is unfamiliar, so the explainer is a real feature, not fluff.
- "No password. Your device is your key." explainer (stay signed in until power-off; reboot → re-enter your handle).
- Philosophy: no servers, no tracking, your data is yours.
- Version (dozenal), feedback email, licences, TOKEN-stack credits.

## Panel shape

Top-level pages rather than one long scroll — **You · Fleet · Security · Recovery · Appearance · Notifications · Updates · Diagnostics · About** — because Fleet and Security each need room to breathe.

## Scaffold status (2026-07-06)

Built and rendering: 9 pages, left-rail nav, real fluor Dropdown / Slider / Textbox + a custom Checkbox, stub pills for the action buttons, orb → Settings entry.
No behaviour wired — controls show visual state only; wire per page as needed, draining `take_change` / `take_toggle` / `take_click` on the release edge.

**Known limitation to fix next:** the region **divisions are correct and fixed** (layout proportions look good and should stay), but **font sizes and element/control sizes are fixed pixels** — they need to scale with the viewport RU / zoom (derive from the same `effective_span` / zoom the chrome uses) so text and controls resize with the window, while the divisions stay exactly as they are.

