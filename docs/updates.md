# Live updates

How a running photon client moves itself to a newer version — hot on desktop (self-replace + re-exec, transparent across the swap), notification-gated on Android (the OS owns package installs).

Status: DESIGN. Nothing here is built yet — this captures the agreed flow so we build against it instead of re-deriving it verbally.

## Goal

Ship new versions to end users without them re-running an installer or hunting for an APK.
On desktop the swap is transparent: the app fetches a newer signed binary, verifies it, replaces itself on disk, and re-execs into the new version, landing on the same screen — the only thing a user notices is that it happened.
On Android the OS forbids an app silently replacing its own package, so the ceiling is a notification that routes to the system installer with one tap.

## The one invariant that matters: verify before exec

The update channel is a remote-code-execution channel pointed straight at every user's device — it is the single highest-value target in the whole system, so the signature check is not a feature, it is the entire security boundary.
Every fetched artefact (manifest and binary alike) is Ed25519-signed by the release key — the same key `photon-keygen` produces and that binaries already self-verify against on startup (`photon-signing-key`).
A binary is verified on disk with that existing self-verify path BEFORE it is ever executed; an artefact that does not verify is discarded and the current version keeps running untouched.

Two properties ride alongside the signature:

- **Monotonic version.** The client refuses any version less than or equal to what it is already running, so a signed-but-old (and possibly vulnerable) build can never be replayed to force a downgrade. Version is compared as an ordered integer, not a string.
- **Provenance stays on the artefact.** The signature covers the whole binary, so a bit-flip in transit or on disk fails the check the same way tampering does — there is no path where a corrupt download runs.

Later hardening (out of scope for v1, noted so it is not forgotten): reproducible builds plus a transparency log, so a maliciously-signed update targeting one user is publicly detectable rather than silent.

## The version manifest and where the binary comes from

A small VSF document, signed by the release key, published to FGTW/R2 next to the binaries.
It carries, per platform/arch: the current version (the ordered integer), the artefact URL, and the artefact's expected hash.
The client polls it, and a manifest whose signature or monotonicity fails is ignored exactly like a bad binary.
The manifest is the only thing the client trusts to learn "a newer version exists"; it never infers freshness from a filename or a directory listing.

For now the artefact URL points at a single dedicated signed executable hosted on fgtw.org (the same R2 the installers already serve from) — one canonical download per platform.
The eventual goal is swarm / BitTorrent-style peer distribution so releases spread without a central host (aligned with the peers-are-fgtw direction), but that is a ways off; the centralised fgtw.org executable is the v1 source and nothing in the verify-before-exec model changes when distribution later decentralises — the signature is what is trusted, not where the bytes came from.

## Settings → Updates

A dedicated Updates page in the settings nav rail:

- **Check for updates** — a manual poll-now button: hit the manifest immediately and, if a newer signed version exists, run the flow now rather than waiting for the automatic cadence. Always present on every platform.
- **Automatic updates** (desktop) — ON by default. The toggle to disable it exists for people who want manual control, and the fact that it defaults on is disclosed in the operating notes (see the terms/disclosures section below) — it is not a silent default.
- **Notify of updates** (Android) — ON by default. Android cannot auto-apply, so this governs whether the "a new version is available" notification appears at all.

## Desktop self-replace (Linux / macOS / Windows)

Desktop updates apply automatically by default — a user who wants manual control turns off "Automatic updates" in Settings → Updates, at which point only the "Check for updates" button (or a disabled auto-path) moves them forward.
That auto-by-default is a deliberate security posture (users on the current signed build, not stragglers on a known-vulnerable one) and is disclosed, not hidden.

1. **Check.** Poll the signed manifest. A newer, higher version → proceed; otherwise do nothing.
2. **Download + verify on disk.** Fetch the new signed binary to a temp path alongside the install location, confirm its hash matches the manifest, then run the Ed25519 self-verify against the file. Only a fully-verified binary advances; anything else is deleted and the running version is untouched.
3. **Atomic swap.**
   - *Unix (Linux/macOS):* `rename()` the verified new binary over the old path. `rename` is atomic, and the running process keeps executing from its already-open inode, so overwriting the path is safe — the old process finishes on the old image, the path now resolves to the new one.
   - *Windows:* the running `.exe` is locked and cannot be overwritten in place. Rename the running exe to a sibling `photon.old` (permitted even while running), write the verified new exe to the original path, and delete `photon.old` on the next launch.
4. **Re-exec.** Replace the running process with the new binary: `execv` on Unix (the process image is swapped in place, same PID), spawn-new-then-exit-old on Windows.
5. **Transparent because state is persisted.** Session roots (the tohu registers), contacts and conversation history (the vault), and presence all persist independently of the process, so the re-exec resumes on the same screen with the same identity — no re-login, no reload prompt.

### In-flight text

The one thing that does not survive a naive re-exec is text a user has typed into a box but not yet sent.
For a genuinely transparent swap, serialise the focused textbox's contents (and cursor/selection) across the exec — a tiny volatile hand-off, distinct from the durable vault — and restore it on the new process's first frame.
Without this the swap is "transparent aside from unsent typing"; with it there is no visible seam at all.

## Android

Android does not let an app overwrite its own installed package — replacing the APK goes thru the system package installer and requires an explicit user action, by OS design.
So the Android path is: with "notify of updates" on (the default), the client learns from the same signed manifest that a newer version exists and surfaces a notification.
On tap it **explains what the update is** (version, and what changed) before asking for anything, then **requests the install-other-apps permission** (`REQUEST_INSTALL_PACKAGES`) if it is not already granted, and only then hands the downloaded, signature-verified APK to the system package installer.
The verify-before-anything invariant still holds — the client checks the APK's signature against the release key before offering to install it — but the final permission grant, install, and restart are the user's taps, not a silent swap.
The order matters: explain first, request permission second, install third — never lead with a bare permission prompt.

## Gating (when the swap is allowed to happen)

The swap must never yank the app out from under someone mid-action, so it is gated on the user being idle: not typing, not mid-conversation, not mid-CLUTCH ceremony.
When a swap has happened, surface it event-shown (a small "updated to vN" note cleared on the next interaction), never on a timer — consistent with the no-time-based-UI rule.
The exact idle definition and whether to offer a manual "update now" affordance are open questions below; v1 can start conservative (only swap from an idle Ready screen) and loosen later.

## What exists today vs. what to build

Exists and is reused:
- Ed25519 binary signing (`photon-keygen`) and startup self-verify — the verification primitive.
- R2 distribution + the FGTW/VSF signed-document machinery — the manifest transport.
- Durable session + vault persistence — what makes the re-exec transparent.

To build:
- The signed version manifest format + its publish step in the release scripts.
- The client poll + verify + monotonic-compare check.
- The platform-split swap (Unix rename, Windows `.old` dance) + re-exec.
- In-flight textbox hand-off across the exec.
- The Android notification → system-installer path.
- Idle gating + the event-shown "updated" note.

## Decided

- **Manual affordance:** yes — a "Check for updates" button on the Settings → Updates page, every platform.
- **Desktop default:** automatic updates ON by default, with an opt-out toggle (disclosed in the operating notes).
- **Android default:** "notify of updates" ON by default; the notification explains, then requests `REQUEST_INSTALL_PACKAGES`, then hands off to the system installer.
- **v1 source:** one dedicated signed executable per platform on fgtw.org; swarm/BitTorrent distribution is a later evolution that does not change the trust model.

## Open questions

- **Poll cadence / trigger.** Startup only, a periodic check, or push (an FGTW event when a release lands, riding the existing WebSocket hub)? Push is cheapest on battery and fastest to roll out, but startup-plus-periodic is simplest to build first. The manual "Check for updates" button exists regardless.
- **Rollback.** If a new version crashes on launch, do we keep the previous binary (the Windows `.old`, a Unix `.prev`) and auto-revert after N failed starts? Worth it for a self-updating app; scope for v1 TBD.
- **Idle definition.** Exactly which states count as "safe to swap" — Ready-and-untouched-for-a-bit, or a broader rule.
