# Sourced, not executed. Host-native desktop build → Ed25519-sign → install to ~/.local/bin. Run on whatever desktop OS you're on; it builds for that host. (Cross-target builds + the real version-bumping ship live in deploy.sh, not here.)
#
# Profiles: `dev`     = --features development (debug profile, debug-info).
#           `release` = --release (PHOTON_ALLOW_RELEASE gate).

build_sign_install() {
    local profile="$1"
    local prof_dir

    # All the cheap source-level ratchets (vsf raw-parse, expired-migration, wrapped-comment, no-python, cpu-feature) in ONE list, run before the build — see scripts/lib/preflight.sh. The same set every publish runs via manifest_begin_dev_publish, so a slip fails identically here and there.
    source "$(dirname "${BASH_SOURCE[0]}")/preflight.sh"
    preflight_gates

    # Source freeze: reflink-snapshot photon + its path-dep closure THIS instant and build from the frozen copy — edits made while the build runs can't tear it. Off-btrfs (or any snapshot failure) builds the live tree exactly as before. Target stays the real ./target (see snapbuild.sh for why that's cache-coherent), so sign + install below are untouched.
    source "$(dirname "${BASH_SOURCE[0]}")/snapbuild.sh"
    local build_dir="."
    if snapbuild_take; then
        build_dir="$SNAPBUILD_ROOT/photon"
        export CARGO_TARGET_DIR="$(pwd)/target"
        echo "Source frozen (reflink snapshot) — edit away, this build won't see it"
    fi

    if [ "$profile" = "release" ]; then
        prof_dir="release"
        export PHOTON_ALLOW_RELEASE=1
        echo "Building release binary..."
        (cd "$build_dir" && cargo build --release)
    else
        prof_dir="debug"
        echo "Building dev binary..."
        (cd "$build_dir" && cargo build --features development)
    fi

    sign_binary "$prof_dir"

    # Install to ~/.local/bin so `photon-messenger` runs the build you just made — same destination as the user installer, no download. Stage-then-rename (atomic on one filesystem): a running instance holds the old inode open, so a plain cp fails "Text file busy", but swapping the directory entry leaves the live process alone and the NEXT launch picks up the new binary.
    local dir="$HOME/.local/bin"
    mkdir -p "$dir"
    install -m755 "target/$prof_dir/photon-messenger" "$dir/photon-messenger.new"
    mv -f "$dir/photon-messenger.new" "$dir/photon-messenger"
    echo "Installed to $dir/photon-messenger"

    # macOS: refresh the .app bundle too, because that is what the Dock, Spotlight and Finder launch.
    # Installing only to ~/.local/bin meant a dev build was built, signed, "installed" -- and every relaunch from the Dock kept running whatever the last RELEASE install or in-app update left behind. That cost a full debugging round: two fixes were reported as still-broken by testing a binary that had neither.
    #
    # ONE fixed path, the same literal the installers use ($HOME/Applications/Photon Messenger.app) -- never a search, never /Applications. macOS keys TCC privacy grants to (code identity, bundle path), so a bundle that moves between builds re-prompts for Local Network exactly like the churning ad-hoc identifier did before sign.sh pinned org.fgtw.photon.
    #
    # The binary is already fully signed at this point (Apple codesign via rcodesign, then the Ed25519
    # append), so it is dropped in as-is: re-signing here would strip the Ed25519 tail and the binary would fail its own startup self_verify.
    #
    # Only refreshes a bundle that ALREADY exists -- building one is the installer's job (it has the
    # Info.plist and the icns). No bundle yet? Run installers/install-development.sh once.
    if [ "$(uname -s)" = "Darwin" ]; then
        local app="$HOME/Applications/Photon Messenger.app"
        if [ -d "$app/Contents/MacOS" ]; then
            # Stage-then-rename, same reason as above: a running instance holds the old inode open.
            install -m755 "target/$prof_dir/photon-messenger" "$app/Contents/MacOS/photon-messenger.new"
            mv -f "$app/Contents/MacOS/photon-messenger.new" "$app/Contents/MacOS/photon-messenger"
            echo "Installed to $app"
        else
            echo "No .app bundle at $app — run installers/install-development.sh once to create it (the Dock launches the bundle, not ~/.local/bin)."
        fi
    fi
}

# Swap the live process for the build that just landed: TERM the running instance (quit is a flush edge since b58312b, so state lands), a bounded wait, then launch the fresh install. macOS launches the .app (the bundle is the stable TCC/notification identity — ~/.local/bin would be a different face); elsewhere the installed binary, skipped when no display is reachable (a headless/SSH build has nowhere to paint).
#
# BRIDGE SUICIDE (field 2026-08-25): when dev.sh is driven over the bridge, the shell running it is a DESCENDANT of the very photon it kills (the bridge PTY is served by photon). The instant photon exits, its PTY hangs up and SIGHUPs dev.sh — so the build + the kill land (they run BEFORE photon dies) but the relaunch never fires: "nuked it, never came back". The comment that used to sit here CLAIMED "bash outlives its dead parent"; it doesn't. So on Linux the whole kill→wait→relaunch runs in a DETACHED systemd --user unit (below): a child of the user manager with zero tie to the bridge PTY, immune to photon's death. dev.sh only QUEUES it (systemd-run returns instantly, while photon is still up) and exits clean; the unit does the swap on its own. Its output goes to the journal, not the terminal.
#
# COMM TRUNCATION (field 2026-08-23, the stacked-instances incident): Linux clips process names to 15 chars and "photon-messenger" is 16, so an exact-name pgrep/pkill silently matches NOTHING there — the reload killed nobody, stacked a fresh instance beside the old one, and the old one kept the relay pipe (answering the bridge with code deleted hours earlier). macOS keeps full names, which is why the Mac test lied that this worked. Match BOTH spellings; each no-ops on the other platform.
photon_alive() {
    pgrep -x photon-messenger >/dev/null 2>&1 || pgrep -x photon-messenge >/dev/null 2>&1
}
photon_signal() {
    pkill "$1" -x photon-messenger 2>/dev/null || true
    pkill "$1" -x photon-messenge 2>/dev/null || true
}

# The reload BODY that must outlive the photon it kills. Runs as a detached systemd --user unit (or a setsid fallback), NEVER inline in dev.sh — see BRIDGE SUICIDE above. Re-sourced by that unit, so it may only lean on this file's own helpers (photon_alive / photon_signal), never on dev.sh's sourced libs.
_reload_detached_body() {
    local name="photon-messenger"
    if photon_alive; then
        photon_signal -TERM
        # WAIT for the graceful exit — up to 60s, checked frequently, and NEVER a SIGKILL: quit flushes the vault, and a KILL landing mid-flush is exactly the mass-uncommitted-writes state that births the manifestus fence wedge (three specimens in four days once the old 3s TERM→KILL escalation started running nightly — Mac plow 422745 and 1110637, Linux 307312). A photon that won't exit in 60s is news the operator must see (in the journal now), not a process to shoot.
        local i
        for i in $(seq 1 200); do
            photon_alive || break
            sleep 0.3
        done
        if photon_alive; then
            echo "Reload: ABORTED — $name is still running 60s after TERM (mid-flush or wedged). Not killing it (a KILL mid-flush wedges the vault). Investigate, stop it yourself, then relaunch."
            return 1
        fi
    fi
    # Launch the fresh install as ITS OWN user unit, a SECOND systemd-run — NOT a plain child of this reload unit, whose cgroup teardown (KillMode=control-group) would take photon down the moment this body returns. The nested run re-parents photon straight to the manager. Fallback (systemd-less box) keeps the old setsid dance with fds 8/9 closed so photon can't inherit the snapshot flock (the 2026-08-24 "5-minute hang").
    if command -v systemd-run >/dev/null 2>&1 && systemd-run --user --collect --quiet --working-directory="$HOME" "$HOME/.local/bin/$name" 2>/dev/null; then
        echo "Reload: $name relaunched"
    else
        (cd "$HOME" && setsid "$HOME/.local/bin/$name" >/dev/null 2>&1 </dev/null 8>&- 9>&- &)
        echo "Reload: $name relaunched (setsid fallback)"
    fi
}

reload_photon() {
    local name="photon-messenger"
    if [ "$(uname -s)" = "Darwin" ]; then
        # macOS is the on-screen dev path (no bridge), so the kill+relaunch stays inline; `open` hands the new instance to launchd, already detached.
        if photon_alive; then
            echo "Reload: stopping the running $name..."
            photon_signal -TERM
            local i
            for i in $(seq 1 200); do
                photon_alive || break
                sleep 0.3
            done
            if photon_alive; then
                echo "Reload: ABORTED — $name is still running 60s after TERM (mid-flush or wedged). Not killing it (a KILL mid-flush wedges the vault). Investigate, stop it yourself, then relaunch."
                return 1
            fi
        fi
        open "$HOME/Applications/Photon Messenger.app"
        echo "Reload: Photon Messenger.app relaunched"
    elif [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
        # Hand the ENTIRE kill→wait→relaunch to a detached user unit so photon's death can't SIGHUP the swap (BRIDGE SUICIDE above). Import the display/session env from HERE — the bridge shell carries what photon was launched with — so the relaunched photon (and the nested systemd-run inside the body) can reach the X server and the user bus. Only forward vars that are actually set, so an unset WAYLAND_DISPLAY on X11 doesn't trip --setenv.
        local lib; lib="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/desktop.sh"
        local envargs=() v
        for v in DISPLAY WAYLAND_DISPLAY XAUTHORITY XDG_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS; do
            [ -n "${!v:-}" ] && envargs+=(--setenv="$v")
        done
        if command -v systemd-run >/dev/null 2>&1 && systemd-run --user --collect --quiet \
                --unit=photon-reload "${envargs[@]}" --working-directory="$HOME" \
                bash -c 'source "$0"; _reload_detached_body' "$lib" 2>/dev/null; then
            echo "Reload: swap handed to a detached unit (survives the bridge) — follow it with: journalctl --user -u photon-reload -f"
        else
            # No systemd: best-effort detach. `trap '' HUP` + setsid so the bridge PTY hangup can't kill the swap; fds 8/9 closed so the reload (and the photon it spawns) can't inherit the snapshot flock.
            ( trap '' HUP; cd "$HOME"; setsid bash -c 'source "$0"; _reload_detached_body' "$lib" >/dev/null 2>&1 </dev/null 8>&- 9>&- & )
            echo "Reload: swap detached (setsid fallback — no systemd-run)"
        fi
    else
        echo "Reload: skipped the relaunch — no display in this environment (the swap is installed; launch from the desktop)"
    fi
}
