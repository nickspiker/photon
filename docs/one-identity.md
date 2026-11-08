# One identity per device, every app

Status: DESIGN, 2026-11-08, nothing built. The owner: "1 means 1": a device holds one identity, and every FGTW app on it (Photon, Mahere, Lumis first) sees it the moment it is there.

## Decisions (Nick 2026-11-08)

- **One identity per device.** An app does not own the identity; it looks for one. If one is present it opens straight into it; if none is, it runs the shared attest, and once that finishes every app on the device sees the identity.
- **Attest is shared, not Photon's.** Today the flow (handle entry, the verdict, mint or join, pairing words) lives in Photon. On ferros the OS already attests and apps are detached from it. Everywhere else it moves out of Photon so any app can call it.
- **An unbound device says so.** A device with no identity shows "This device is unbound: tap to attest", leading to bind (join an existing identity) or mint (a fresh one).
- **One About page across the apps,** which is also the dashboard: what is attached to this identity, and where to find more passless apps.
- **No store. Open.** Discover is not a marketplace and has no gatekeeper.

## Device states

| state | what is true | what every app shows |
|---|---|---|
| Unbound | no fleet lists this device | "This device is unbound": tap to attest, then bind or mint |
| Locked | a vault exists for this device, no session since boot | "Re-enter your handle" (stay signed in until power-off, docs/settings-panel.md) |
| Live | the session registers are present | the app itself |

The verdict that tells Unbound from Locked is fgtw's one pure function (`fgtw::attest::attest_verdict`), already shared with ferros's bare-metal Photon.

## Is an identity present?

- **Desktop (Linux, Windows, macOS): already one per login.** `tohu::session()` reads the 96-byte registers `{identity_seed, vault_seed, handle_proof}` from `$XDG_RUNTIME_DIR/tohu` (or the private `tohu-<uid>` directory where there is no XDG runtime dir). Any app built on tohu reads the same file, so the second app finds the first app's session today.
- **Android: one per app today.** The session is a boot-locked capsule in the app's own files, and the sandbox keeps it there. Two facts help:
  - apps signed with our key share `ANDROID_ID` (it is per signer), so they compute the same key root and are the same fleet device: attest in a second app resolves to Member, never to a new device;
  - but the session itself is not shared, so a second app would ask for the handle again.
  The fix is a channel only same-signer apps can open (a signature-level permission on a small provider or bound service in each app): an app asks its siblings for a live session before showing Locked. Open decision below.
- **ferros:** the OS holds it (vault common area, init); an app never attests.

## What moves out of Photon

1. **The attest network path** (`network/handle_query.rs`: the seed query, LAN `fleet_req` / `fleet_resp`, the verdict, mint, join and pairing) into fgtw, beside `fgtw::attest`, so Mahere and Lumis call the same code.
2. **The screens** (handle entry, the lamps, the permanence warning and retype, pairing words) into one shared fluor component, so every app looks and behaves alike. Its crate is to be named.
3. **Photon keeps** everything after Live: conversations, fleet, the rest.

## The About page and dashboard

One shared component, embedded by every app, in three sections:

- **About:** the passless explainer ("No password. Your device is your key."), the identity card (published name, avatar, devices and their assurance level), and this app's version, licences and credits.
- **Attached:** every app bound to the identity, with what it may touch (its grants) and **Detach**.
  - On ferros, removing it removes the kist (docs/KISTS.md in ferros).
  - Elsewhere, Detach takes the app off the identity and marks its data for discard; uninstalling stays the OS's job, and the page can open the OS uninstaller.
- **Discover:** passless apps to add.

**The record** is ferros's `kists` document (ferros docs/KISTS.md: one `kist` section per app, capsule hash, developer key and option, name, icon, `wants` with each grant and the person's answer and its Eagle time), the same schema on every OS. On ferros it lives in the vault's common area. Elsewhere it would ride fleet state (sealed with the fleet key, device to device, fgtw.org only as the backup), so an app attached on one device shows on all of them. That is new state, so it waits on the owner's OK (AGENT.md Rule -1).

**Discover is open.** It is a signed VSF list like the release manifest: each entry a developer key, a capsule hash, a name, an icon and where to fetch it. The list only points; trust comes from the developer's own signature over the capsule, checked on the device, so anyone can publish an app and anyone can publish a list.

## Hardening the trust

Every process on a device can read its plate source (Widevine ID, machine-id, IOPlatformUUID), and every same-user process can read the desktop session file. The options beyond that, a crypto tag in the pocket acting as PIPE, and kernel enforcement, are in tohu docs/oracle-hardening.md, "Beyond the oracle".

## Open decisions

- **The Android session channel:** a signature-permission provider in each app, or one app as the holder?
- **The shared screens' crate:** its name and home (beside fgtw, or a crate of its own).
- **The `kists` record on fleet state** (Rule -1).
- **Lumis:** its interface is Kotlin, not fluor: port it, or draw the page natively from shared Rust data?
- **The tab's name:** "Apps" for people, "kist" kept internal?

## Order

1. Lift the attest path into fgtw; Photon calls it unchanged.
2. The shared attest screens, including Unbound; Photon uses them.
3. Mahere: identity present → open; absent → the shared attest.
4. The `kists` record and the About / Attached page.
5. Discover.
6. Lumis.
