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

    # Apple targets first get the STABLE-identity Apple code signature (self-signed 10-yr cert, identifier org.fgtw.photon) — macOS TCC keys privacy grants on it, so updates stop re-prompting Local Network. Must precede the Ed25519 append (rcodesign rewrites the Mach-O). Skipped gracefully when the tooling/cert isn't on this box.
    case "$target" in
        *apple-darwin*)
            # Resolve the keys dir the same way keystore.sh does rather than hardcoding the desktop's mount — the MacBook keeps it at ~/Code/keys, and a missing cert here silently downgrades to an ad-hoc signature (TCC then re-prompts for Local Network on every update).
            local cert=""
            for d in /mnt/Octopus/Code/keys /mnt/Chiton/MEGA/Code/keys "$HOME/MEGA/code/keys" "$HOME/Code/keys"; do
                if [ -f "$d/photon-macos-codesign.crt" ]; then
                    cert="$d/photon-macos-codesign"
                    break
                fi
            done
            if command -v rcodesign >/dev/null && [ -f "$cert.crt" ]; then
                rcodesign sign \
                    --pem-file "$cert.crt" \
                    --pem-file "$cert.key" \
                    --binary-identifier org.fgtw.photon \
                    "$bin"
            else
                echo "WARN: rcodesign/cert missing — shipping ad-hoc Apple signature (TCC will re-prompt per update)"
            fi
            ;;
    esac

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
