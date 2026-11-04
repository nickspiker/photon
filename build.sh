#!/bin/bash
# Build Photon from source and run it — the one command a fresh clone needs.
#
#   ./build.sh               fetch any missing sibling repos, build the development profile, run it
#   ./build.sh --no-run      the same, without launching
#   ./build.sh --version N   rebuild release vN from exactly the source it shipped from, in a snapshot under target/pinned/vN
#
# Photon builds against a family of sibling crates by path dependency (vsf, fluor, fgtw, …), each its own repo cloned beside this one in the same parent folder.
# This script clones any that are missing — the list comes from the Cargo.toml path graph itself, so it never goes stale — then builds.
# The development profile needs no signing keys: an unsigned development build skips the startup self-check that release builds enforce.
# Siblings are cloned from https://github.com/nickspiker/<name> unless PHOTON_SIBLINGS_BASE names another base URL (a fork's org).
# To bring existing siblings up to date, run scripts/pull-deps.sh.
#
# --version N (security audit UPW-012): a release's tag message records the commit of every sibling it was built from.
# The siblings beside this checkout are shared with other projects and never move; instead each is checked out at its recorded commit as a git WORKTREE under target/pinned/vN, photon beside them at the tag, and the build runs there.
# The result is the same source every byte of vN came from (the toolchain and the C libraries behind the codecs still move with your machine).

set -e

run=1
version=""
while [ $# -gt 0 ]; do
    case "$1" in
        --no-run) run=0 ;;
        --version) shift; version="${1:-}"; [ -n "$version" ] || { echo "build.sh: --version needs a release number" >&2; exit 2; }; run=0 ;;
        -h|--help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "build.sh: unknown option $1 (try --help)" >&2; exit 2 ;;
    esac
    shift
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

if [ -z "$version" ]; then
    cargo build --features development
    echo
    echo "Built: $PHOTON/target/debug/photon-messenger"
    if [ "$run" = 1 ]; then
        exec "$PHOTON/target/debug/photon-messenger"
    fi
    exit 0
fi

# A pinned rebuild: the tag names photon's commit, its message names every sibling's.
tag="v$version"
git fetch --quiet --tags origin 2>/dev/null || true
git rev-parse -q --verify "refs/tags/$tag" >/dev/null || { echo "build.sh: no tag $tag in this repo (git fetch --tags origin)" >&2; exit 1; }
msg="$(git tag -l --format='%(contents)' "$tag")"
full_version="$(printf '%s\n' "$msg" | sed -n -E 's/^release v[0-9]+ \(([0-9]+\.[0-9]+\.[0-9]+),.*/\1/p' | head -n 1)"
if ! printf '%s\n' "$msg" | grep -q '^sibling '; then
    echo "build.sh: tag $tag records no sibling commits — releases before 2026-11-03 cannot be rebuilt pinned" >&2
    exit 1
fi
snap="$PHOTON/target/pinned/$tag"
echo "Pinned rebuild of $tag in $snap"
mkdir -p "$snap"
# photon itself at the tag's commit.
if [ ! -d "$snap/photon" ]; then
    git worktree add --quiet --detach "$snap/photon" "$tag"
fi
# Every sibling the tag names, at its recorded commit; a sibling missing beside this checkout is cloned first.
while read -r word name hash; do
    [ "$word" = sibling ] || continue
    if [ ! -d "$ROOT/$name/.git" ]; then
        printf '  %-12s cloning\n' "$name"
        git clone --quiet "$BASE/$name" "$ROOT/$name"
    fi
    if ! git -C "$ROOT/$name" cat-file -e "$hash^{commit}" 2>/dev/null; then
        git -C "$ROOT/$name" fetch --quiet origin || true
        git -C "$ROOT/$name" cat-file -e "$hash^{commit}" 2>/dev/null || { echo "build.sh: $name has no commit $hash" >&2; exit 1; }
    fi
    if [ ! -d "$snap/$name" ]; then
        git -C "$ROOT/$name" worktree add --quiet --detach "$snap/$name" "$hash"
    fi
    printf '  %-12s %s\n' "$name" "${hash:0:12}"
done <<< "$msg"
# The tag's tree holds the pre-release version (tag-authority); the shipped number comes from the tag message, injected exactly as deploy.sh did.
if [ -n "$full_version" ]; then
    sed -i -E "s/^version = \"[0-9]+\.[0-9]+\.[0-9]+\"/version = \"$full_version\"/" "$snap/photon/Cargo.toml"
fi
echo
( cd "$snap/photon" && cargo build --features development )
echo
echo "Built: $snap/photon/target/debug/photon-messenger (source of $tag; remove the snapshot with: git worktree prune after rm -rf $snap)"
