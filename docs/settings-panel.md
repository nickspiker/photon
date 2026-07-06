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
- **Retire this device** — see Security; reachable from the device row.
- **Device sidecar**: each device keeps its own device-local preferences.
  Split: **device-local** = this device's name, notification volume, appearance, its log.
  **Fleet-wide (synced)** = identity, contacts, avatar, party colours.
  The device-local sidecar is a device-keyed store that must NOT propagate through the synced vault.

### Security
Named by **destructiveness** — three distinct actions:
- **Lock** — zeroize the in-RAM seed, keep the vault ciphertext on disk. The manual version of the reboot boundary: re-unlock with your handle. (This is `clear_session()` + zeroize; half-built already.)
- **Retire this device** — remove a device from the fleet.
  On **this** device (selling it): unbind from the fleet chain + offer a **Shred**.
  On a **sibling** (remotely, from another device): a fleet-chain **revoke** — it may be offline/already sold, so it can't be shredded, only barred from rejoining/decrypting. The remote revoke is the "I sold it and forgot to wipe" safety net.
- **Shred** — crypto-shred: zeroize the keys (data is already ciphertext, so the dying key makes it unrecoverable), then delete files and let the OS reclaim. Distinct from Retire, pairs with it for handoff. A plain "wipe local storage, re-fetch from fleet" belongs here too — format bumps (e.g. vsf v7→v9) make a clean re-fetch genuinely useful.
- Sec/Recovery posture strip.

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
- **Non-Android** (Linux / macOS / Windows / Redox): **"Install updates automatically" — on by default.** Linux self-replaces its own binary with no gate; macOS/Windows are promptless *per update* provided the build is signed/notarised (a one-time pipeline cost already paid in `deploy.sh`, not a per-update tap).
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

