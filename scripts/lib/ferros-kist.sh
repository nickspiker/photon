# Sourced, not executed. Photon for ferros devices: the kist capsule deploy.sh builds, signs and publishes beside the other platforms (docs/updates.md, "ferros").
# The build is ferros's, from its repo beside this one: std/photon is Photon's library under fluor's ferros host, built by ferros's tools/photon-build.sh (nightly, the ferros-rust std fork, the ferros target JSON, ferros_rt), and ferros's tools/photon-capsule.sh signs the flat image into a capsule carrying Photon's declaration (its wants, in slot order) and its option (ferros docs/KISTS.md). The declaration lives only there, so a development install and a release can never declare different wants.
# The capsule is signed with Photon's release key (owner, 2026-11-10: Photon is a commons project, so its kist carries the key that signs its manifest and binaries, not ferros's key), so the capsule's developer key (`ke`) is AUTHOR_PUBKEY. The bridge reads a key only in its VSF form (ferros_kist::secret: section developer_key { label, seed kge }; it refuses a raw key file), so the kist is signed with photon-signing-key.vsf, the same release seed converted once from the raw photon-signing-key (2026-11-12, ferros tools/kistcat `devkey convert --label PHOTON_RELEASE`: the label whose Falcon-512 and SPHINCS+ domains are SigningBundle::release's, so the bundle is the one fgtw pins); every other signer in this repo still reads the raw file thru PHOTON_SIGNING_KEY. The key's PATH (PHOTON_KIST_KEY) is handed to ferros-bridge as FERROS_DEV_KEY; the key itself is read only inside the bridge's process, never copied or printed.
# Callers cd to the repo root first. Every function `return 1`s on failure with a line saying what, so the caller's set -e and traps abort the deploy: a release ships every platform or none (the Windows ARM64 rule), so a missing ferros toolchain fails the deploy, never skips the target.

FERROS_DIR="${FERROS_DIR:-$(cd .. && pwd)/ferros}"
# Photon's kist option: its identity across versions (ferros ORACLE.md §5), minted 2026-11-08, never changed. ferros's photon-capsule.sh writes it; ferros_kist_sign refuses a capsule that carries any other, so a changed option can never ship (it would make every person's Photon a new kist, their answers and its vault domain gone).
FERROS_KIST_OPTION="8c2411c6bf1c10a45d8021340b2dae4a7c69774235069eddd85abbdc6959eb74"
# The profile: `debug` is photon-build.sh's development build (Photon at opt-level 1, its dependencies at 3, std/photon's Cargo.toml), the one every ferros device runs today. `release` has never run on a device; switching is the owner's call, and this one word is the switch.
FERROS_KIST_PROFILE="debug"
# Where the image and the capsule land: the deploy's own target dir (CARGO_TARGET_DIR there), its own triple, beside every other platform's artefact.
FERROS_KIST_TARGET="${FERROS_KIST_TARGET:-$(pwd)/target}"
FERROS_KIST_IMAGE="$FERROS_KIST_TARGET/aarch64-unknown-ferros/$FERROS_KIST_PROFILE/photon.bin"
FERROS_KIST_CAPSULE="$FERROS_KIST_TARGET/aarch64-unknown-ferros/$FERROS_KIST_PROFILE/photon.capsule.vsf"
# ferros-rust (the std fork) and ferros-libc are named by absolute path in photon-build.sh and std/photon's Cargo.toml; checked here, recorded in the provenance below.
FERROS_RUST_DIR="/mnt/Harbor/Code/ferros-rust"
FERROS_LIBC_DIR="/mnt/Harbor/Code/ferros-libc"
# Photon's release key in the VSF form the bridge reads (the header above); never the raw file, which the bridge refuses.
PHOTON_KIST_KEY="${PHOTON_KIST_KEY:-/mnt/Harbor/Code/keys/photon-signing-key.vsf}"

# Everything the ferros build needs, checked in a second before any platform builds, so a missing piece fails the deploy at its start and not an hour in.
ferros_kist_preflight() {
    local missing="" f t
    for f in std/photon/Cargo.toml tools/photon-build.sh tools/photon-capsule.sh targets/aarch64-unknown-ferros.json tools/std-sysroot/include tools/ferros-bridge/Cargo.toml; do
        [ -e "$FERROS_DIR/$f" ] || missing="$missing\n    $FERROS_DIR/$f"
    done
    [ -d "$FERROS_RUST_DIR/library/std" ] || missing="$missing\n    $FERROS_RUST_DIR/library (the ferros-rust std fork)"
    [ -f "$FERROS_LIBC_DIR/Cargo.toml" ] || missing="$missing\n    $FERROS_LIBC_DIR (libc with a ferros arm)"
    for t in aarch64-linux-gnu-gcc aarch64-linux-gnu-ar aarch64-linux-gnu-objcopy; do
        command -v "$t" >/dev/null 2>&1 || missing="$missing\n    $t (binutils-aarch64-linux-gnu, gcc-aarch64-linux-gnu)"
    done
    cargo +nightly --version >/dev/null 2>&1 || missing="$missing\n    the nightly Rust toolchain (rustup toolchain install nightly)"
    [ -f "$PHOTON_KIST_KEY" ] || missing="$missing\n    $PHOTON_KIST_KEY (Photon's release key as VSF: ferros tools/kistcat devkey convert --raw <keys>/photon-signing-key --label PHOTON_RELEASE --secret <keys>/photon-signing-key.vsf)"
    if [ -n "$missing" ]; then
        echo "ERROR: the ferros toolchain is incomplete; Photon for ferros (the kist capsule) is a required target. Missing:"
        printf '%b\n' "$missing"
        echo "       Clone ferros, ferros-rust and ferros-libc beside photon (ferros docs/STD-TARGET.md), or remove the ferros target from deploy.sh."
        return 1
    fi
}

# Freeze std/photon with the rest of the source: a copy placed in the snapshot at <root>/ferros/std/photon, where its `../../../photon` is the snapshot's Photon (the version-injected, frozen tree), never the live one. With no root (the snapshot did not take) the live std/photon builds, against the live tree, as every other platform does then.
# Also reads what the ferros build stands on, for the tag: ferros (std/photon, ferros_rt, the target JSON, the scripts), ferros-rust, ferros-libc, each HEAD, `+dirty` when that repo has changes the build reads. ferros_rt and the std fork build from their live trees (named by absolute path inside ferros-rust), so their commits are what this records.
ferros_kist_freeze() {
    local root="$1" entry
    FERROS_KIST_PKG="$FERROS_DIR/std/photon"
    if [ -n "$root" ]; then
        mkdir -p "$root/ferros/std/photon" || return 1
        for entry in "$FERROS_DIR/std/photon"/* "$FERROS_DIR/std/photon"/.[!.]*; do
            { [ -e "$entry" ] || [ -L "$entry" ]; } || continue
            [ "${entry##*/}" = target ] && continue
            cp -a --reflink=auto "$entry" "$root/ferros/std/photon/" || return 1
        done
        FERROS_KIST_PKG="$root/ferros/std/photon"
    fi
    FERROS_KIST_FROM="ferros $(ferros_kist_rev "$FERROS_DIR" std/photon ferros_rt targets tools/std-sysroot tools/photon-build.sh tools/photon-capsule.sh)"$'\n'"ferros-rust $(ferros_kist_rev "$FERROS_RUST_DIR" library)"$'\n'"ferros-libc $(ferros_kist_rev "$FERROS_LIBC_DIR" .)"
    return 0
}

# A repo's HEAD for the provenance, `+dirty` when the named paths have changes; `uncommitted` for a repo with no commit yet (ferros-rust, 2026-11-10: its whole library is staged, never committed), so the tag says so instead of the deploy dying on it.
ferros_kist_rev() {
    local dir="$1" h
    shift
    h="$(git -C "$dir" rev-parse -q --verify HEAD 2>/dev/null)" || h="uncommitted"
    if [ -n "$(git -C "$dir" status --porcelain -- "$@" 2>/dev/null)" ]; then
        h="$h+dirty"
    fi
    echo "$h"
}

# Build ferros-bridge (the capsule signer, a host tool in ferros's own target dir) and the image, from the frozen std/photon. Each failure names itself: a bare subshell failing inside a function does not fire the ERR trap (deploy.sh's snap_cargo note).
ferros_kist_build() {
    local args=()
    [ "$FERROS_KIST_PROFILE" = "release" ] && args=(--release)
    ( cd "$FERROS_DIR" && env -u CARGO_TARGET_DIR cargo build -p ferros-bridge ) || { echo "^^^ BUILD FAILED: ferros-bridge (in $FERROS_DIR), the capsule signer ^^^" >&2; return 1; }
    PHOTON_FERROS_PKG="$FERROS_KIST_PKG" CARGO_TARGET_DIR="$FERROS_KIST_TARGET" "$FERROS_DIR/tools/photon-build.sh" "${args[@]}" || { echo "^^^ BUILD FAILED: Photon for ferros ($FERROS_KIST_PKG) ^^^" >&2; return 1; }
    [ -s "$FERROS_KIST_IMAGE" ] || { echo "ERROR: no image at $FERROS_KIST_IMAGE after the ferros build" >&2; return 1; }
}

# Sign the image into the capsule with the release key, then check it with Photon's own trust root: every release egg verifies, the content hash holds, the kist section's option is Photon's. A capsule that fails is never in the manifest.
# Arg: the photon-manifest tool (it carries AUTHOR_PUBKEY).
ferros_kist_sign() {
    local tool="$1"
    rm -f "$FERROS_KIST_CAPSULE"
    FERROS_DEV_KEY="$PHOTON_KIST_KEY" "$FERROS_DIR/tools/photon-capsule.sh" "$FERROS_KIST_IMAGE" "$FERROS_KIST_CAPSULE" || { echo "ERROR: signing the ferros capsule failed" >&2; return 1; }
    "$tool" --check-capsule "$FERROS_KIST_CAPSULE" "$FERROS_KIST_OPTION" || { echo "ERROR: the ferros capsule is not Photon's (key or option); refusing to ship it" >&2; return 1; }
}
