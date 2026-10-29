#!/bin/bash
# Build Photon from source and run it — the one command a fresh clone needs.
#
#   ./build.sh            fetch any missing sibling repos, build the development profile, run it
#   ./build.sh --no-run   the same, without launching
#
# Photon builds against a family of sibling crates by path dependency (vsf, fluor, fgtw, …), each its own repo cloned beside this one in the same parent folder.
# This script clones any that are missing — the list comes from the Cargo.toml path graph itself, so it never goes stale — then builds.
# The development profile needs no signing keys: an unsigned development build skips the startup self-check that release builds enforce.
# Siblings are cloned from https://github.com/nickspiker/<name> unless PHOTON_SIBLINGS_BASE names another base URL (a fork's org).
# To bring existing siblings up to date, run scripts/pull-deps.sh.

set -e

run=1
for arg in "$@"; do
    case "$arg" in
        --no-run) run=0 ;;
        -h|--help) sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "build.sh: unknown option $arg (try --help)" >&2; exit 2 ;;
    esac
done

PHOTON="$(cd "$(dirname "$(readlink -f "$0" 2>/dev/null || echo "$0")")" && pwd)"
ROOT="$(cd "$PHOTON/.." && pwd)"
BASE="${PHOTON_SIBLINGS_BASE:-https://github.com/nickspiker}"
cd "$PHOTON"

need() {
    command -v "$1" >/dev/null 2>&1 || { echo "build.sh: $1 is not installed — $2" >&2; exit 1; }
}
need git "install git"
need cargo "install Rust from https://rustup.rs"
need cmake "install CMake (the Opus voice codec is compiled from its vendored source)"

# Walk the path dependencies from photon outward, cloning each sibling that is missing.
# An optional dependency is left alone when its repo cannot be fetched: optional means photon's build does not enable it (opsin's closed calibration crate is one).
echo "Siblings in $ROOT:"
todo="photon"
seen=""
while [ -n "$todo" ]; do
    cur="${todo%% *}"
    case "$todo" in *" "*) todo="${todo#* }" ;; *) todo="" ;; esac
    case " $seen " in *" $cur "*) continue ;; esac
    seen="$seen $cur"
    while IFS= read -r line; do
        rel="$(printf '%s\n' "$line" | sed -n 's/.*path[[:space:]]*=[[:space:]]*"\.\.\/\([^"/]*\)[^"]*".*/\1/p')"
        [ -n "$rel" ] || continue
        case " $seen $todo " in *" $rel "*) continue ;; esac
        if [ ! -f "$ROOT/$rel/Cargo.toml" ]; then
            if [ -e "$ROOT/$rel" ]; then
                echo "build.sh: $ROOT/$rel exists but holds no Cargo.toml — move it aside and run this again" >&2
                exit 1
            fi
            printf '  %-12s cloning\n' "$rel"
            if ! git clone --quiet "$BASE/$rel" "$ROOT/$rel" 2>/dev/null; then
                case "$line" in
                    *optional*true*) printf '  %-12s skipped (optional, not fetchable)\n' "$rel"; continue ;;
                    *) echo "build.sh: could not clone $BASE/$rel" >&2; exit 1 ;;
                esac
            fi
        fi
        todo="$todo $rel"
    done < <(grep 'path[[:space:]]*=[[:space:]]*"\.\./' "$ROOT/$cur/Cargo.toml")
done
for r in $(printf '%s\n' $seen | sort); do
    [ "$r" = photon ] || printf '  %-12s %s\n' "$r" "$(git -C "$ROOT/$r" log --oneline -1 2>/dev/null | cut -c1-60)"
done
echo

cargo build --features development
echo
echo "Built: $PHOTON/target/debug/photon-messenger"
if [ "$run" = 1 ]; then
    exec "$PHOTON/target/debug/photon-messenger"
fi
