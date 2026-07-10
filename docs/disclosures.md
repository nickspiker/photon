# Disclosures — how Photon behaves

Photon has no company behind it — no entity offers you a service, holds your data, or signs a contract with you.
So there are no Terms & Conditions in the usual sense: a contract needs a counterparty, and there isn't one.
What there is instead is this document: a plain statement of how the software behaves, so you can decide whether to run it.
Running Photon is the acceptance; everything it does that a reasonable person would want to know about is disclosed here, and everything adjustable says where the switch is.

Status: DESIGN alongside `docs/updates.md`. This is the source text; a condensed version surfaces in-app under Settings → About.

## Your identity is permanent

There is no password, no reset, and no recovery.
Attesting a handle mints cryptographic roots derived from it; the first human to attest owns that identity, and nothing can give it to anyone else — including you, if you lose all your devices without having added another one.
Devices are replaceable thru the fleet (add a device while you still have one; remove any device from another); the identity itself is not.
Handles are labels and can change; the identity underneath cannot.

## Your data lives on your devices

Messages, contacts, and history are stored encrypted on your own devices and nowhere else.
There is no server that holds your conversations, and no one — including whoever operates fgtw.org — can read them.

What does touch fgtw.org (the bootstrap server), and what it sees:

- **Attestation and presence** — your handle proof (a one-way hash, not your handle), a device public key, and your IP address and port, so peers can find you.
  This is how a peer-to-peer network works; the address book is public by design, the contents of your conversations never pass thru it.
- **Encrypted fleet state** — your contact roster and fleet membership, sealed so only your own devices can open them.
- **Avatars** — encrypted per-handle; the server stores bytes it cannot decode.
- **Diagnostic logs, only if you press Submit** — see below.

## Updates apply automatically (desktop)

On Linux, macOS, and Windows, Photon checks a signed manifest and updates itself to newer releases by default — download, verify the signature on disk, swap, re-exec, transparently.
This is a deliberate security posture: an unpatched messenger is a worse risk to you than an automatic one, so the default keeps you on the current signed build.
If you want manual control, turn off Automatic updates in Settings → Updates; the Check-for-updates button is always there.
Every update is verified against the release signing key before a single instruction of it runs, and a version can never silently move backwards.
On Android the operating system does not permit self-update; Photon notifies you (on by default, Settings → Updates), explains what changed, and hands the verified package to the system installer — installing is always your tap.

## Diagnostic logs are opt-in and sealed

Photon keeps a small on-device log (16 MiB cap, self-expiring, no names — identifiers are pseudonymous fingerprints).
It leaves your device only when you press Submit on the Diagnostics page, and it is encrypted before it leaves: sealed under a key derived from your identity, so only someone who knows your handle can find or read it.
That is the point of the feature — you hand your handle to whoever is helping you debug, and that act is what grants them access to what you submitted.
Clear wipes the log at any time.

## What you are trusting

Honesty about the trust surface, because "trustless" is a lie in any real system:

- **The release signing key.** Updates are signature-gated code execution; whoever holds the release key can ship you code. That is the whole reason verification-before-execution is the update system's central invariant.
- **Your own devices.** Keys are derived on-device and never leave it; a device you lose or that is compromised is trusted until you remove it from your fleet (Settings → Fleet → Remove), which rotates keys away from it.
- **The fgtw.org bootstrap** for peer discovery — it can see the public address book and could refuse service, but it cannot read messages, forge your identity, or push you code.
  The long-term direction is to retire it into the peer swarm itself.

## Leaving

Settings → Security → Shred wipes this device to a blank slate: vault destroyed, session cleared, ready for a fresh identity or a new owner.
Removing a device from your fleet cuts it off cryptographically — it cannot decrypt anything sealed after its removal.
There is no account to delete because there is no account; there is no data to request back because no one else has it.

## Licences

Photon is built on the TOKEN stack; component licences ride under the hood and are listed in-app.
