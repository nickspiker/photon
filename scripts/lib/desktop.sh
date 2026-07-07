# Sourced, not executed. Host-native desktop build → Ed25519-sign → install to ~/.local/bin.
# Run on whatever desktop OS you're on; it builds for that host. (Cross-target builds + the real
# version-bumping ship live in deploy.sh, not here.)
#
# Profiles: `dev`     = --features development (debug profile, debug-info).
#           `release` = --release (PHOTON_ALLOW_RELEASE gate).

build_sign_install() {
    local profile="$1"
    local prof_dir

    # Hand-rolled-VSF ratchet: block the build if a network-facing file grew a raw parse site.
    source "$(dirname "${BASH_SOURCE[0]}")/vsf-gate.sh"
    vsf_gate

    if [ "$profile" = "release" ]; then
        prof_dir="release"
        export PHOTON_ALLOW_RELEASE=1
        echo "Building release binary..."
        cargo build --release
    else
        prof_dir="debug"
        echo "Building dev binary..."
        cargo build --features development
    fi

    sign_binary "$prof_dir"

    # Install to ~/.local/bin so `photon-messenger` runs the build you just made — same destination
    # as the user installer, no download. Stage-then-rename (atomic on one filesystem): a running
    # instance holds the old inode open, so a plain cp fails "Text file busy", but swapping the
    # directory entry leaves the live process alone and the NEXT launch picks up the new binary.
    local dir="$HOME/.local/bin"
    mkdir -p "$dir"
    install -m755 "target/$prof_dir/photon-messenger" "$dir/photon-messenger.new"
    mv -f "$dir/photon-messenger.new" "$dir/photon-messenger"
    echo "Installed to $dir/photon-messenger"
}
