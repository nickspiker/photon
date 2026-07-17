# Sourced, not executed. Source freeze for builds: the moment the build starts, photon + its entire path-dep closure reflink-copy (btrfs CoW — metadata only, no data copied, same-filesystem instant) into a STABLE snapshot dir, and cargo builds from the frozen tree — so editing continues fearlessly in the live tree without tearing the running build.
#
# Why the pieces are the way they are:
# - STABLE path (Code/.build-snap, recreated each run, never $$-suffixed): a path-dep package's identity IS its path, so a stable snap path keeps its fingerprints valid across runs.
# - CARGO_TARGET_DIR stays the REAL photon/target: snap-path and live-path units carry different unit hashes, so both worlds coexist in one target dir (crates.io deps shared outright — only the path crates hold two resident copies), and the built binary lands exactly where sign_binary/install expect.
# - The snapshot is destroyed on EVERY exit path (trap EXIT) — success, failure, ctrl-C; a fresh one is taken per build. Cargo.lock churn inside the snapshot vanishes with it.
# - Each crate's target/ is skipped (irrelevant + huge); sibling .git dirs are skipped; photon's .git rides along because build.rs stamps the commit + dirty state — the snapshot's dirty state IS the frozen moment's truth.
# - Any failure (not btrfs, cp error, no flock) → snapbuild_take returns 1 and the caller builds the live tree exactly as before. Graceful, never load-bearing.

SNAPBUILD_CODE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/.."
SNAPBUILD_ROOT="$SNAPBUILD_CODE_ROOT/.build-snap"

# photon + the full path-dep closure, computed from the Cargo.tomls at snapshot time (walk `path = "../X"` entries to a fixed point) — a hardcoded list rots the day a crate gains a dep.
snapbuild_crates() {
    local pending=(photon) done_list=() c dep
    while [ ${#pending[@]} -gt 0 ]; do
        c="${pending[0]}"
        pending=("${pending[@]:1}")
        case " ${done_list[*]} " in *" $c "*) continue ;; esac
        done_list+=("$c")
        [ -f "$SNAPBUILD_CODE_ROOT/$c/Cargo.toml" ] || continue
        for dep in $(grep -o 'path = "\.\./[A-Za-z0-9_-]*"' "$SNAPBUILD_CODE_ROOT/$c/Cargo.toml" | sed 's|.*"\.\./||; s|"||'); do
            pending+=("$dep")
        done
    done
    echo "${done_list[@]}"
}

snapbuild_take() {
    command -v flock >/dev/null 2>&1 || return 1
    # One snapshot at a time per box; fd 8 stays open for the script's life (fd 9 is the publish lock).
    exec 8>>"$SNAPBUILD_ROOT.lock" 2>/dev/null || return 1
    flock 8 || return 1
    rm -rf "$SNAPBUILD_ROOT" || return 1
    mkdir -p "$SNAPBUILD_ROOT" || return 1
    local c entry base
    for c in $(snapbuild_crates); do
        [ -d "$SNAPBUILD_CODE_ROOT/$c" ] || continue
        mkdir "$SNAPBUILD_ROOT/$c" || { snapbuild_drop; return 1; }
        for entry in "$SNAPBUILD_CODE_ROOT/$c"/* "$SNAPBUILD_CODE_ROOT/$c"/.[!.]*; do
            { [ -e "$entry" ] || [ -L "$entry" ]; } || continue
            base="${entry##*/}"
            case "$base" in
                target) continue ;;
                .git) [ "$c" = photon ] || continue ;;
            esac
            cp -a --reflink=always "$entry" "$SNAPBUILD_ROOT/$c/" 2>/dev/null || { snapbuild_drop; return 1; }
        done
    done
    trap snapbuild_drop EXIT
    return 0
}

snapbuild_drop() {
    rm -rf "$SNAPBUILD_ROOT"
}
