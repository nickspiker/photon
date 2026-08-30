#!/bin/sh
# Photon resilient launcher — see docs/resilient-launch.md.
# Two independent copies of the signed binary are installed; try each in fixed priority, verify its Ed25519 signature (bounded), and exec the first that passes. If none do, tell the user to reinstall.
# Corruption/availability resilience ONLY: this shim is itself an unverified file the OS runs directly, so it defends against bit-rot, a nuked folder, or a torn update — never a local attacker who could just replace the shim too.
# Paths + timeout are env-overridable: install-time mount detection points COPY_B at a second disk where one exists, and the test suite points both at fixtures.
A="${PHOTON_COPY_A:-$HOME/.local/bin/photon-messenger}"
B="${PHOTON_COPY_B:-$HOME/.local/share/photon/photon-messenger}"
T="${PHOTON_VERIFY_TIMEOUT:-8}"

# `timeout` bounds the one corruption shape running-it can't report cleanly: a copy that HANGS. Stock macOS ships no `timeout`; there we run the verify unbounded (a hang is far rarer than a nuke or bit-flip, which the signature check still catches).
if command -v timeout >/dev/null 2>&1; then VERIFY="timeout $T"; else VERIFY=""; fi

for c in "$A" "$B"; do
    [ -x "$c" ] || continue
    # `photon verify` reads the file, checks its own appended signature, and exits 0 (valid) / non-zero (invalid) doing nothing else.
    if $VERIFY "$c" verify >/dev/null 2>&1; then
        # Exactly one validation: hand off with PHOTON_LAUNCH_VERIFIED so the launched copy skips its now-redundant startup self-check (it consumes the flag so it can't leak onward). `exec` so no launcher process lingers under the app.
        PHOTON_LAUNCH_VERIFIED=1 exec "$c" "$@"
    fi
done

msg="Photon: every installed copy is missing or failed verification — reinstall from https://holdmyoscilloscope.com/photon"
command -v notify-send >/dev/null 2>&1 && notify-send "Photon Messenger" "$msg" 2>/dev/null
printf '%s\n' "$msg" >&2
exit 1
