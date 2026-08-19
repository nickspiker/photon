# Sourced, not executed. Host-native desktop build → Ed25519-sign → install to ~/.local/bin. Run on whatever desktop OS you're on; it builds for that host. (Cross-target builds + the real version-bumping ship live in deploy.sh, not here.)
#
# Profiles: `dev`     = --features development (debug profile, debug-info).
#           `release` = --release (PHOTON_ALLOW_RELEASE gate).

build_sign_install() {
    local profile="$1"
    local prof_dir

    # Hand-rolled-VSF ratchet: block the build if a network-facing file grew a raw parse site.
    source "$(dirname "${BASH_SOURCE[0]}")/vsf-gate.sh"
    vsf_gate

    # Expired-compatibility-branch ratchet: a migration names the release it dies in, and this fails the build once the tree reaches it. See scripts/lib/migration-gate.sh.
    source "$(dirname "${BASH_SOURCE[0]}")/migration-gate.sh"
    migration_gate

    # Wrapped-comment ratchet: one line per thought, never hard-wrapped — covers photon, the fgtw path-dep, and the build scripts, zero baseline.
    source "$(dirname "${BASH_SOURCE[0]}")/comment-gate.sh"
    comment_gate

    # NO-PYTHON ratchet: this is a Rust tree — no build script may shell out to python (use a Rust bitty-executable instead). Lives with comment-gate.sh; sourced above.
    no_python_gate

    # CPU-feature ratchet: a dependency may not silently opt us into instructions our oldest device lacks (a Snapdragon 855 SIGILLed on ML-KEM's aarch64 Keccak assembly, 2026-08-01). See scripts/lib/arch-gate.sh.
    source "$(dirname "${BASH_SOURCE[0]}")/arch-gate.sh"
    arch_gate

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
