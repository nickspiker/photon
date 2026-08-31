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
        case "$target" in
            *-windows-*) bin="$bin.exe" ;;
        esac
    else
        bin="target/$profile/photon-messenger"
        # A no-target build is a HOST build, so on macOS it is an apple-darwin build — say so, or the codesign case below never matches. It didn't: `dev.sh` -> `desktop.sh` calls `sign_binary <profile>`
        # with no target, so every native Mac dev build skipped Apple signing and fell through to an ad-hoc signature whose identifier is derived from the binary CONTENT (observed:
        # `photon_messenger-81be93e6fc198260`). That means a NEW identity on every rebuild, while the release bundle carries the stable `org.fgtw.photon`. macOS TCC keys privacy grants to the code identity, so a churning identifier re-prompts for Local Network on every build and cannot hold onto identity-scoped state like an NSStatusItem.
        if [ "$(uname -s)" = "Darwin" ]; then
            target="$(uname -m | sed 's/^arm64$/aarch64/')-apple-darwin"
        fi
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
            for d in /mnt/Harbor/Code/keys /mnt/Chiton/MEGA/Code/keys "$HOME/MEGA/code/keys" "$HOME/Code/keys"; do
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
        # The allow flag is for THIS helper tool only, not a photon release: build.rs refuses any bare --release, and on an outside builder's box that refusal would kill the whole dev run before the graceful no-key skip below could.
        PHOTON_ALLOW_RELEASE=1 cargo build --release --bin photon-signature-signer || true
        signer="target/release/photon-signature-signer"
    fi

    echo "Signing $bin..."
    if "$signer" "$bin"; then
        echo "✓ Signed"
    elif [ "$profile" = "release" ]; then
        echo "ERROR: signing failed — a release binary must carry the Ed25519 signature (self_verify hard-gates it)."
        exit 1
    else
        # An outside builder has no signing key; dev builds waive the self-check (src/crypto/self_verify.rs), so an unsigned dev binary runs fine.
        echo "! Signing skipped (no signing key on this box) — unsigned DEVELOPMENT build, self-verify waived."
    fi
}
