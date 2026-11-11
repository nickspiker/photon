# One identity per device, every app

Status: DESIGN, 2026-11-08, nothing built; "Plate and papers" below carries the trust model. The owner: "1 means 1": a device holds one identity, and every FGTW app on it (Photon, Mahere, Lumis first) sees it the moment it is there.

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
3. **The fleet flows too** (owner 2026-11-10): add, join, depart, lock, chain news, fleet state, as machines in `fgtw::flow` (fgtw FLOWS.md), so ferros inherits the behaviour instead of re-implementing it.
4. **Photon keeps** the messenger: conversations, CLUTCH, history, waves and beams. On ferros it has no fleet page; elsewhere it draws the fleet screens from fgtw's machines.

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

## Plate and papers (owner direction 2026-11-08)

The owner: "leave the plate on each one, the best we can, then put the cert in the glovebox."

- **The plate** is the device's public name, the best source each platform has: Widevine on Android, the TPM endorsement key on a desktop with a TPM, `machine-id` / `MachineGuid` / `IOPlatformUUID` otherwise (tohu docs/oracle-hardening.md). Anyone can read it and that is fine: it names the hardware, it proves nothing. fgtw.org already ties each plate to the identity that claimed it (one plate, one identity, fgtw-bootstrap).
- **The papers** are a key made inside the device's secure hardware and never exportable, registered in the fleet chain: StrongBox on Android, the TPM on a desktop, the Secure Enclave on a Mac. Not a stored secret anyone can copy; the device's registration, and a reset destroys it.

### A reset device belongs to its owner

- A factory reset destroys the papers and leaves the plate, still claimed by the owner's identity.
- So the device cannot be signed into any other fleet ("because I OWN IT"), and it cannot sign itself back into its owner's either.
- The owner's tap adds it back: a sibling sponsors it, or the owner's tag does, and the device makes new papers for the glovebox.

### Adding a device: green is proof

Pairing v2 already works this way (docs/pairing-v2.md, steps 7 to 9): a bind is keyless until the owner confirms, only the device actually bound goes green, and the fleet key is sealed only after the confirm. The owner: "only one device will turn green, and I'm not going to click add until I see it do so."

- **Papers make green proof.** Without them a cloned key (a desktop's key is rebuilt from its machine-id today) lights a clone beside the real device, and the confirm seals the fleet key to a key the attacker holds. With papers only one device can hold the key, so only one can light.
- **The typed words can retire from the network path.** They pick the right request out of the registry's list; with trustworthy green, the list and the owner's eyes on the lamp do that, and a wrong pick leaves only a keyless ledger entry.
- **The plate puts the right row first.** A reset device's plate is already the owner's, so the sponsor shows it as that device coming back; a new device's plate is unclaimed, and the green check covers it.

### Contention shuts the network path

The owner: "if you do get an attacker and your old reset device you'd have two devices so you'd just completely disable that option and require BLE or NFC presence, like a traditional device bind."

- **More than one request for the identity in the window** closes the registry path for that add: the sponsor's screen offers only devices it can hear over BLE (and NFC once built), pairing v2's proximity rule ("proximity populates the tap list, never the registry").
- **It stays closed** until that add finishes, so an attacker cannot withdraw and repost to reopen it.
- **The attempt is shown to the owner.** A second request means someone else holds the handle, so the sponsor says so plainly (and the handle-rename path, project memory "Handle rename", is the follow-up).

### The tag

- **A one-device owner** who resets that device has no sibling to tap, so the tag is enrolled in the chain as an owner credential, able to sponsor a device back in, not only as a second factor.
- The tag's shape (FIDO2 `hmac-secret`) and why NTAG 424 DNA does not fit: tohu docs/oracle-hardening.md, "A tag as PIPE".

### Selling a device

A plate claimed forever makes a sold phone a brick for its buyer. Removing a device needs a title transfer: the owner signs the plate over, or releases it, as part of the removal.

### Decided: papers overrule the keystore rule

The owner, 2026-11-08: "I think we have to overrule for papers." The 2026-11-02 rule against keystores (tohu docs/oracle-hardening.md rule 1) stays for the plate and for any copyable stored secret; the papers, a key the hardware will not export, are the exception. Using them locally puts no vendor in the loop; checking a vendor's attestation chain would, and stays out.

## Open decisions

- **The papers feed the device key.** StrongBox holds an HMAC key, so the device secret becomes the chip's HMAC over a fixed label, the TPM HMAC root's twin and PIPE's 32 in, 32 out. That changes the frozen `v0` derivation: every device moves once, as Android did on 2026-11-02.
- **Uninstall on Android.** Keystore keys die with the app, not only with a factory reset (whether a data clear kills them too is to be checked), so a reinstall needs a tap too, undoing the reinstall-resumes behaviour that fixed Jeff's phone (2026-11-04).
- **Desktop papers that die on reset.** The TPM HMAC root survives an OS reinstall. To match Android, the key would be the TPM's HMAC over the machine-id: it needs the chip, and it changes when the OS is reinstalled.
- **The Android session channel:** a signature-permission provider in each app, or one app as the holder? Keystore keys belong to one app, so the papers point to one holder answering for the others.
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

The trust track runs beside it, once the open decisions are ruled:

1. Contention shuts the network path (works on today's keys; the deterrent first).
2. Desktop papers: the TPM key root fed into the device key, with the move every desktop makes once.
3. Android papers in StrongBox, the same move.
4. Green-only add: the typed words leave the network path.
5. The tag as owner credential, and the title transfer.
