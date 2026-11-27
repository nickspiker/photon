# Sourced, not executed. Photon for ferros devices: the kist capsule deploy.sh builds, signs and publishes beside the other platforms (docs/updates.md, "ferros").
# The build is ferros's, from its repo beside this one: std/photon is Photon's library under fluor's ferros host, built by ferros's tools/photon-build.sh (nightly, the std fork in ferros forks/rust, the ferros target JSON, ferros_rt), and ferros's tools/photon-capsule.sh signs the flat image into a capsule carrying Photon's declaration (its wants, in slot order) and its option (ferros docs/KISTS.md). The declaration lives only there, so a development install and a release can never declare different wants.
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
# The capsule's entry in the kists directory (ferros KIST-DISTRIBUTION.md 1.0), signed with the same key and written beside it: what lists the capsule on kists.fgtw.org, so ferros devices offer the update.
FERROS_KIST_ENTRY="$FERROS_KIST_TARGET/aarch64-unknown-ferros/$FERROS_KIST_PROFILE/photon.capsule.entry.vsf"
# The first ferros forge commit with the entry form (photon-capsule.sh --entry, 25abc81c) and `kistcat publish` with the append endpoint (7376db93).
FERROS_KIST_ENTRY_FORGE="7376db93"
# The std fork and libc with a ferros arm live inside ferros (forks/rust, forks/libc, since ferros 81658181): std/photon names libc relatively and the fork's std names ferros_rt relatively, so whichever ferros tree builds, live or frozen, builds against its own forks.
# Photon's release key in the VSF form the bridge reads (the header above); never the raw file, which the bridge refuses.
PHOTON_KIST_KEY="${PHOTON_KIST_KEY:-/mnt/Harbor/Code/keys/photon-signing-key.vsf}"

# Everything the ferros build needs, checked in a second before any platform builds, so a missing piece fails the deploy at its start and not an hour in.
ferros_kist_preflight() {
    local missing="" f t
    for f in std/photon/Cargo.toml tools/photon-build.sh tools/photon-capsule.sh targets/aarch64-unknown-ferros.json tools/std-sysroot/include tools/ferros-bridge/Cargo.toml; do
        [ -e "$FERROS_DIR/$f" ] || missing="$missing\n    $FERROS_DIR/$f"
    done
    [ -d "$FERROS_DIR/forks/rust/library/std" ] || missing="$missing\n    $FERROS_DIR/forks/rust/library (the std fork)"
    [ -f "$FERROS_DIR/forks/libc/Cargo.toml" ] || missing="$missing\n    $FERROS_DIR/forks/libc (libc with a ferros arm)"
    for t in aarch64-linux-gnu-gcc aarch64-linux-gnu-ar aarch64-linux-gnu-objcopy; do
        command -v "$t" >/dev/null 2>&1 || missing="$missing\n    $t (binutils-aarch64-linux-gnu, gcc-aarch64-linux-gnu)"
    done
    cargo +nightly --version >/dev/null 2>&1 || missing="$missing\n    the nightly Rust toolchain (rustup toolchain install nightly)"
    git -C "$FERROS_DIR" merge-base --is-ancestor "$FERROS_KIST_ENTRY_FORGE" HEAD 2>/dev/null || missing="$missing\n    ferros forge $FERROS_KIST_ENTRY_FORGE or later (the entry form and kistcat publish; git -C $FERROS_DIR pull)"
    [ -f "$FERROS_DIR/tools/kistcat/Cargo.toml" ] || missing="$missing\n    $FERROS_DIR/tools/kistcat (the entry and the publish)"
    [ -f "$PHOTON_KIST_KEY" ] || missing="$missing\n    $PHOTON_KIST_KEY (Photon's release key as VSF: ferros tools/kistcat devkey convert --raw <keys>/photon-signing-key --label PHOTON_RELEASE --secret <keys>/photon-signing-key.vsf)"
    if [ -n "$missing" ]; then
        echo "ERROR: the ferros toolchain is incomplete; Photon for ferros (the kist capsule) is a required target. Missing:"
        printf '%b\n' "$missing"
        echo "       Clone ferros beside photon (ferros docs/STD-TARGET.md), or remove the ferros target from deploy.sh."
        return 1
    fi
}

# Freeze ferros's source with the rest: its whole working tree (tracked and untracked, never what git ignores: target/, the disk images), reflinked into the snapshot at <root>/ferros, where std/photon's `../../../photon` is the snapshot's Photon (the version-injected, frozen tree), never the live one.
# The whole tree, not std/photon alone: std/photon names ferros crates by path (ferros_fluor, ferros_sys and theirs), and those inherit from ferros's workspace root, which cargo loads with every member (v157's first deploy, 2026-11-24: a snapshot holding std/photon alone failed on ferros_fluor).
# A repo beside ferros that a crate names by `../../<repo>` (tatau) is carried the same way (ferros_kist_carry_siblings).
# With no root (the snapshot did not take) the live std/photon builds, against the live tree, as every other platform does then.
# Also reads what the ferros build stands on, for the tag: ferros (its tree, forks included, its memories aside) and each repo carried beside it, each HEAD, `+dirty` when that repo has changes the build reads.
# The std source is the frozen tree's own forks/rust/library (FERROS_KIST_STD_SRC, handed to photon-build.sh as __CARGO_TESTS_ONLY_SRC_ROOT), so std, ferros_rt and libc all come from the one snapshot and the one ferros rev names them.
ferros_kist_freeze() {
    local root="$1"
    FERROS_KIST_PKG="$FERROS_DIR/std/photon"
    FERROS_KIST_STD_SRC="$FERROS_DIR/forks/rust/library"
    FERROS_KIST_CARRIED=""
    if [ -n "$root" ]; then
        ferros_kist_carry "$FERROS_DIR" "$root/ferros" || return 1
        ferros_kist_carry_siblings "$root" || return 1
        FERROS_KIST_PKG="$root/ferros/std/photon"
        FERROS_KIST_STD_SRC="$root/ferros/forks/rust/library"
    fi
    FERROS_KIST_FROM="ferros $(ferros_kist_rev "$FERROS_DIR" . ':(exclude)Claude memories' ':(exclude)CLAUDE MEMORY.md')${FERROS_KIST_CARRIED}"
    return 0
}

# One repo's source as its working tree holds it, tracked and untracked but never what git ignores, reflinked to `dest` with its layout kept.
ferros_kist_carry() {
    local src="$1" dest="$2"
    mkdir -p "$dest" || return 1
    (
        set -o pipefail
        cd "$src" && git ls-files -z -co --exclude-standard \
            | while IFS= read -r -d '' f; do
                { [ -e "$f" ] || [ -L "$f" ]; } && printf '%s\0' "$f"
            done \
            | xargs -0 -r cp -a --reflink=auto --parents -t "$dest"
    ) || { echo "ERROR: could not freeze $src into the snapshot at $dest"; return 1; }
}

# Every path dependency the frozen std/photon reaches, directly or thru another crate, that the snapshot lacks: its live repo is carried whole, beside the others as it sits beside them live.
# Absolute paths name live trees by design and are left alone; a `path` that names a source file ([lib], [[bin]]) holds no Cargo.toml and is passed over.
ferros_kist_carry_siblings() {
    local root code dir p dep live top seen=" "
    # Resolved like every dependency below: the deploy names the snapshot `photon/../.build-snap`, and a root left unresolved matches no resolved path, so every repo was passed over (v157's second deploy, 2026-11-24: tatau).
    root="$(realpath -m "$1")"
    local queue=("$root/ferros/std/photon")
    code="$(cd "$FERROS_DIR/.." && pwd)"
    while [ "${#queue[@]}" -gt 0 ]; do
        dir="${queue[0]}"
        queue=("${queue[@]:1}")
        case "$seen" in *" $dir "*) continue ;; esac
        seen="$seen$dir "
        while IFS= read -r p; do
            case "$p" in /*) continue ;; esac
            dep="$(realpath -m "$dir/$p")"
            case "$dep" in "$root"/*) ;; *) continue ;; esac
            if [ ! -f "$dep/Cargo.toml" ]; then
                live="$code/${dep#"$root"/}"
                [ -f "$live/Cargo.toml" ] || continue
                top="$(git -C "$live" rev-parse --show-toplevel)" || { echo "ERROR: $live is in no git repo; the snapshot cannot carry it"; return 1; }
                ferros_kist_carry "$top" "$root/${top#"$code"/}" || return 1
                FERROS_KIST_CARRIED="$FERROS_KIST_CARRIED"$'\n'"${top##*/} $(ferros_kist_rev "$top" .)"
            fi
            queue+=("$dep")
        done < <(grep -vE '^[[:space:]]*#' "$dir/Cargo.toml" | grep -oE '(^|[^A-Za-z0-9_])path *= *"[^"]+"' | sed -E 's/.*"([^"]+)"/\1/')
    done
    return 0
}

# A repo's HEAD for the provenance, `+dirty` when the named paths have changes; `uncommitted` for a repo with no commit yet (the std fork, 2026-11-10, before it moved into ferros: its whole library staged, never committed), so the tag says so instead of the deploy dying on it.
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
    __CARGO_TESTS_ONLY_SRC_ROOT="$FERROS_KIST_STD_SRC" PHOTON_FERROS_PKG="$FERROS_KIST_PKG" CARGO_TARGET_DIR="$FERROS_KIST_TARGET" "$FERROS_DIR/tools/photon-build.sh" "${args[@]}" || { echo "^^^ BUILD FAILED: Photon for ferros ($FERROS_KIST_PKG) ^^^" >&2; return 1; }
    [ -s "$FERROS_KIST_IMAGE" ] || { echo "ERROR: no image at $FERROS_KIST_IMAGE after the ferros build" >&2; return 1; }
}

# Sign the image into the capsule with the release key, its directory entry beside it, then check the capsule with Photon's own trust root: every release egg verifies, the content hash holds, the kist section's option is Photon's. A capsule that fails is never in the manifest. Its declaration carries the version this deploy ships (FULL_VERSION, the one injected into the snapshot), so a ferros device lists it and offers the update (ferros KISTS.md "The version").
# Arg: the photon-manifest tool (it carries AUTHOR_PUBKEY).
ferros_kist_sign() {
    local tool="$1"
    rm -f "$FERROS_KIST_CAPSULE" "$FERROS_KIST_ENTRY"
    FERROS_DEV_KEY="$PHOTON_KIST_KEY" PHOTON_VERSION="$FULL_VERSION" "$FERROS_DIR/tools/photon-capsule.sh" "$FERROS_KIST_IMAGE" "$FERROS_KIST_CAPSULE" --entry "$FERROS_KIST_ENTRY" || { echo "ERROR: signing the ferros capsule or writing its entry failed" >&2; return 1; }
    [ -s "$FERROS_KIST_ENTRY" ] || { echo "ERROR: no entry at $FERROS_KIST_ENTRY after signing the capsule" >&2; return 1; }
    "$tool" --check-capsule "$FERROS_KIST_CAPSULE" "$FERROS_KIST_OPTION" || { echo "ERROR: the ferros capsule is not Photon's (key or option); refusing to ship it" >&2; return 1; }
}

# List the capsule in the kists directory: its entry appended at kists.fgtw.org (`kistcat publish`, which checks the entry as a device would first), once the capsule and the entry are live on the kists bucket.
# The answer is printed (taken, or refused and why); a refusal returns 1 and names the command that sends it again.
ferros_kist_publish() {
    ( cd "$FERROS_DIR" && cargo run -q --release --manifest-path tools/kistcat/Cargo.toml -- publish "$FERROS_KIST_ENTRY" ) && return 0
    echo "  ✗ the kists directory did not take Photon's entry; the release is live but ferros devices are not offered it. Send it again:" >&2
    echo "      (cd $FERROS_DIR && cargo run -q --release --manifest-path tools/kistcat/Cargo.toml -- publish $FERROS_KIST_ENTRY)" >&2
    return 1
}
