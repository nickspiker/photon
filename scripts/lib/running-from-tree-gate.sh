# Sourced, not executed. Refuse to build/publish while a photon process is EXECUTING from this repo tree.
#
# This should be impossible (Nick 2026-09-03: "Photon should NEVER be running from the preflight dir, that would be very bad") — the installed copy lives in ~/.local/bin or the app dir, never in target/. If this gate trips, something is genuinely wrong: someone launched the binary straight out of target/, or a stale instance survived a botched install. Either way the build is about to rewrite the file underneath a live process.
#
# Why it matters even tho Linux "protects" a running executable: the signer does an in-place `fs::write` (truncate + write) on the freshly linked binary, and cargo's linker replaces it outright. A running instance holding that path is how you get a mid-deploy ETXTBSY three minutes in, or a half-signed artifact — and on the build box that instance is usually the BRIDGE, i.e. the shell running the deploy commits suicide.
running_from_tree_gate() {
    local root exe pids=""
    # `git rev-parse` for the tree root, not BASH_SOURCE arithmetic: sourced-file path juggling resolved against $PWD and pointed the gate at $HOME, where it flagged every editor and language server on the box. The gate always runs inside the checkout, so git is both simpler and authoritative. No git (tarball build) = nothing to protect, pass.
    root="$(git rev-parse --show-toplevel 2>/dev/null)" || return 0
    [ -n "$root" ] || return 0

    case "$(uname -s)" in
        Linux)
            # /proc is the authority: the exe symlink is the real path of the running image, immune to argv games.
            for p in /proc/[0-9]*; do
                exe="$(readlink "$p/exe" 2>/dev/null)" || continue
                case "$exe" in
                    "$root"/*) pids="$pids ${p#/proc/}($exe)" ;;
                esac
            done
            ;;
        Darwin)
            # No /proc, and macOS `comm` echoes the launch path VERBATIM — a process started as `target/foo` reports `target/foo`, so absolute matching alone silently misses the exact case this gate exists for (caught by a positive-control probe, 2026-09-03). Match absolute paths under the tree AND bare relative build-dir paths. Best-effort by design: Linux is where deploys run and /proc/exe is authoritative there.
            while read -r pid cmd; do
                case "$cmd" in
                    "$root"/*|target/*|./target/*) pids="$pids $pid($cmd)" ;;
                esac
            done < <(ps -axo pid=,comm= 2>/dev/null)
            ;;
        *) return 0 ;;
    esac

    if [ -n "$pids" ]; then
        echo "GATE: a process is running from INSIDE the repo tree — refusing to build over it." >&2
        echo "      tree: $root" >&2
        for p in $pids; do echo "      pid $p" >&2; done
        echo "      Photon must never run from the build tree: the release build relinks and re-signs these exact files." >&2
        echo "      On the build box that process is usually the bridge — this build would rewrite the binary it is executing." >&2
        echo "      Fix: stop it and relaunch from the installed copy (~/.local/bin/photon-messenger), not from target/." >&2
        return 1
    fi
    return 0
}
