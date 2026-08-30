# Resilient launch

## Goal

Survive corruption or loss of a photon binary — a bit-rotted file, a half-written update, a nuked folder — by installing two independent copies and launching a verified-good one.
This is corruption/availability resilience ONLY.

Tampering is explicitly out of scope.
The launch shim is itself an unverified file the OS runs directly, so anyone who can rewrite a photon copy can rewrite the shim, and no scheme at this layer changes that.
At-rest tamper-defense would require a protected/read-only root (signed-boot territory) and is not attempted here.
Real tamper-defense stays where it already works: the updater verifies a download's signature before it is ever installed or run.

## The two copies

Every desktop platform installs the full signed binary to TWO distinct locations, chosen as independent as the machine allows — ideally different mounts/disks, otherwise different directory roots so a nuked folder never takes both.
Android is the exception: the OS package manager owns the install and restart, so there is one system-managed copy and no shim.

Proposed per-platform layout (copy B prefers a detected second mount; otherwise the path below):
- Linux: copy A = `~/.local/bin/photon-messenger`, copy B = `~/.local/share/photon/photon-messenger`. The `.desktop` `Exec` points at the shim.
- Windows: copy A = `%LOCALAPPDATA%\Programs\PhotonMessenger\photon-messenger.exe`, copy B = a second dir (e.g. `%LOCALAPPDATA%\PhotonMessenger\photon-messenger.exe`). The Start-menu shortcut points at the shim.
- macOS: copy A = the Mach-O inside `~/Applications/Photon Messenger.app/Contents/MacOS`, copy B = `~/.local/bin/photon-messenger`. TCC keys privacy grants to the bundle path, so the bundle MUST stay copy A and the primary launch target — see the TCC note.

Copies are tried in fixed priority (A, then B); no version comparison.
Both are normally the same version, so B is reached only when A is unreadable — and "older but valid" beats "nothing".

## The shim

A tiny launcher — a shell script on Linux/macOS, a small equivalent on Windows — is what the OS launcher entry points at instead of photon directly.
It picks a good copy by one validation, then hands off:

```sh
for c in "$COPY_A" "$COPY_B"; do
    if timeout 8 "$c" verify >/dev/null 2>&1; then
        PHOTON_LAUNCH_VERIFIED=1 exec "$c" "$@"
    fi
done
notify "Photon: all copies failed verification — reinstall from the website"
exit 1
```

- `photon verify` already exists (src/main.rs) and does exactly one job: check this binary's own appended Ed25519 signature, then exit 0 (valid) or non-zero (invalid). It is reused verbatim — no new flag.
- `timeout 8` is the fallback for the one corruption shape running-it does not cleanly report: a copy that hangs instead of crashing or cleanly failing the signature check. Eight seconds is generous for a signature read of a ~44 MB file on a slow disk, and short enough that a wedged copy hands off without a visible stall.
- `exec` replaces the shim, so no launcher process lingers under the running app.

## One validation

The signature is validated exactly once — in the shim's `photon verify` pass.
The subsequent launch must NOT re-validate.

The shim signals this by setting `PHOTON_LAUNCH_VERIFIED=1` in the environment of the launch `exec` only.
photon, at startup, when `PHOTON_LAUNCH_VERIFIED` is set:
- skips the startup self_verify (the shim already did it microseconds earlier on the exact same file), and
- immediately removes the variable from its own environment (`std::env::remove_var`).

The removal is load-bearing, not hygiene.
photon inherits its environment (and forwards its arguments) into child processes — including the self-update re-exec that relaunches a freshly-downloaded binary.
If the skip signal leaked into that re-exec, an update would install and run WITHOUT verifying the new bytes, silently defeating the signed-binary model.
Consuming the variable the instant it is read confines the skip to the single shim→photon hop that earned it.

A direct launch — someone running the binary from a terminal, no shim — sets no such variable, so startup self_verify runs exactly as today; the corruption tripwire stays in place off the shim path.

## macOS / TCC note

macOS binds privacy grants (Local Network, notifications, etc.) to the launched binary's code identity AND bundle path.
So the GUI launch must remain the `.app` bundle (copy A), and the shim must preserve that identity rather than launching a foreign path.
Two ways to resolve in implementation, to be decided when we build the macOS arm:
- place the shim INSIDE the bundle as the `Info.plist` `CFBundleExecutable`, so the bundle identity is what runs, and the shim then execs the real Mach-O (copy A) or falls back to copy B, or
- keep the bundle executable as photon itself and drive the two-copy fallback from photon's own early startup on macOS only.

This is the one platform where the shim-in-front model needs care; Linux and Windows are straightforward.

## photon changes required

Small and contained:
1. Honour `PHOTON_LAUNCH_VERIFIED` at startup: skip the startup self_verify block (around src/main.rs:61) and `remove_var` it immediately. Resolve the downstream use of `signature_hex` on the skip path (today it is derived from the verify result).
2. Nothing else — `photon verify` already exists and is reused as-is.

## Installer / updater changes

- Installer: write both copies, plus the shim and the launcher entry (`.desktop` / shortcut / bundle) pointing at the shim; detect a second mount for copy B where available.
- Updater (`apply_desktop_blocking`): stage-then-rename into BOTH copy paths, not one; the re-exec target is the shim (or copy A). This builds on the canonical-path fix (`installed_exe_path`), so `current_exe()` is never the install target.

## Implementation status

- Linux: FULL — two copies (`~/.local/bin` + `~/.local/share/photon`), the shim, `.desktop` → shim, updater dual-install + re-exec via shim, dev.sh dogfoods it. Shipped.
- macOS: dual-copy only — the installer already writes two copies (`~/.local/bin` + the `.app` bundle) and the updater keeps both fresh, but the Dock still launches the bundle directly (no shim) pending the TCC decision above.
- Windows: dual-copy only — the installer writes a second copy under `%LOCALAPPDATA%\PhotonMessenger` and the updater keeps both fresh, but the Start-menu shortcut still launches the primary directly (a no-console .vbs/.cmd shim is the remaining piece, and it needs a real Windows box to test).
- Android: n/a (system-managed).

So auto-fallback-on-corruption is live on Linux; macOS and Windows get redundancy (survive a nuked/corrupt copy via the other, kept fresh by the updater) but launch the primary directly until their shims are built and tested on-device.

## Threat model, stated plainly

- Defends: corruption, bit-rot, a torn update write, one folder/disk gone.
- Does NOT defend: a local actor with write access to the install location (they replace the shim with anything and it runs — no layer here stops that).
- Tamper-in-transit stays defended by the updater verifying downloads before install.
