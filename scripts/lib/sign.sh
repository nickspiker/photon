# Sourced, not executed. Signs a built photon-messenger binary with the Ed25519
# photon-signature-signer (the same self_verify gate the binary checks at startup).
# Folded from the old root sign-after-build.sh; the one place signing lives now.
# Callers cd to the repo root first.

# sign_binary <debug|release> [target]
#   no target   -> host binary at target/<profile>/photon-messenger
#   with target -> target/<target>/<profile>/photon-messenger  (.exe for windows-gnu)
sign_binary() {
    local profile="$1" target="$2" bin
    if [ -n "$target" ]; then
        bin="target/$target/$profile/photon-messenger"
        if [ "$target" = "x86_64-pc-windows-gnu" ]; then
            bin="$bin.exe"
        fi
    else
        bin="target/$profile/photon-messenger"
    fi
    if [ ! -f "$bin" ]; then
        echo "ERROR: binary not found: $bin"
        exit 1
    fi

    # The signer is a host tool. Prefer the one this build already produced; otherwise build it once (release).

    local signer="target/$profile/photon-signature-signer"
    if [ ! -f "$signer" ]; then
        signer="target/release/photon-signature-signer"
    fi
    if [ ! -f "$signer" ]; then
        echo "Building signature signer (one-time)..."
        cargo build --release --bin photon-signature-signer
        signer="target/release/photon-signature-signer"
    fi

    echo "Signing $bin..."
    "$signer" "$bin"
    echo "✓ Signed"
}
