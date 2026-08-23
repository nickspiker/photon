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

# Swap the live process for the build that just landed: TERM the running instance (quit is a flush edge since b58312b, so state lands), a short bounded wait, KILL any holdout, then launch the fresh install detached from this terminal. macOS launches the .app (the bundle is the stable TCC/notification identity — ~/.local/bin would be a different face); elsewhere the installed binary, skipped when no display is reachable (a headless/SSH build has nowhere to paint). Running THIS thru the bridge works — the swap completes because bash outlives its dead parent — but the command's output dies with the old process; expect silence, then the new instance's presence.
#
# COMM TRUNCATION (field 2026-08-23, the stacked-instances incident): Linux clips process names to 15 chars and "photon-messenger" is 16, so an exact-name pgrep/pkill silently matches NOTHING there — the reload killed nobody, stacked a fresh instance beside the old one, and the old one kept the relay pipe (answering the bridge with code deleted hours earlier). macOS keeps full names, which is why the Mac test lied that this worked. Match BOTH spellings; each no-ops on the other platform.
photon_alive() {
    pgrep -x photon-messenger >/dev/null 2>&1 || pgrep -x photon-messenge >/dev/null 2>&1
}
photon_signal() {
    pkill "$1" -x photon-messenger 2>/dev/null || true
    pkill "$1" -x photon-messenge 2>/dev/null || true
}
reload_photon() {
    local name="photon-messenger"
    if photon_alive; then
        echo "Reload: stopping the running $name..."
        photon_signal -TERM
        local i
        for i in 1 2 3 4 5 6 7 8 9 10; do
            photon_alive || break
            sleep 0.3
        done
        photon_signal -KILL
    fi
    if [ "$(uname -s)" = "Darwin" ]; then
        open "$HOME/Applications/Photon Messenger.app"
        echo "Reload: Photon Messenger.app relaunched"
    elif [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
        # Launch from ~, in a subshell so this script's cwd stays put — without it the relaunched photon inherits the REPO as cwd (dev.sh cd'd here) and bequeaths it to every bridge shell it spawns.
        (cd "$HOME" && setsid "$HOME/.local/bin/$name" >/dev/null 2>&1 </dev/null &)
        echo "Reload: $name relaunched"
    else
        echo "Reload: skipped the relaunch — no display in this environment (the swap is installed; launch from the desktop)"
    fi
}
