#!/bin/bash
# Fast-forward photon AND every sibling repo it actually depends on, so a build/deploy can't mix a fresh photon with a stale sibling.
#
# WHY THIS EXISTS: `for d in ~/Code/*/` walks ~85 repos, most of which photon never touches — slow, and it pulls unrelated work you may not want moving. The dependency CLOSURE is 12 repos, and it is derived here from the Cargo.toml `path = "..."` graph rather than hardcoded, so it stays right when deps change. Cargo's git deps (the [patch.crates-io] winit/softbuffer/audiopus_sys forks) live in ~/.cargo/git and are cargo's problem, not this script's — the local winit-patched checkout is deliberately NOT in the graph.
#
# A stale sibling is the "missing fgtw/fluor symbol" class: photon compiles against a sibling that lacks the symbol its new code calls, producing dozens of phantom errors that look like photon's fault. Fast-forward everything together or nothing.
#
# THE OTHER HALF: scripts/lib/sibling-push-gate.sh (sibling_push_check) guards the PUSH direction — work stranded on this machine, unpushed or uncommitted. This script guards the PULL direction — work missing from this machine, behind or diverged. Same repo set, opposite failure. Keep both; neither is redundant.
#
# EXPECT A LOCAL COMMIT AFTER EVERY PUBLISH: the publish flow commits the version line itself ("dev: <target> vX published; next line vY"), so the box that published always holds a commit no other machine has. That is normal, not corruption — pull --rebase and push it. It is also why a build box diverges from a laptop that pushed meanwhile, with nobody having touched a keyboard.
#
# Exit status is deploy-gateable: 0 = every repo current and clean, non-zero = at least one repo needs a human.

set -u
# Resolve the tree from THIS SCRIPT's own location, never from $HOME or a hardcoded ~/Code: the build box wipes its home directory daily, so the checkout lives on a persistent mount whose path is nobody's business but the filesystem's. readlink -f so invoking through a symlink still lands on the real tree.
SELF="$(readlink -f "$0" 2>/dev/null || echo "$0")"
PHOTON="$(cd "$(dirname "$SELF")/.." && pwd)"
ROOT="$(cd "$PHOTON/.." && pwd)"   # the parent that holds every sibling checkout

if [ ! -d "$PHOTON/.git" ]; then
    echo "pull-deps: $PHOTON is not a git checkout — is this script running from inside the photon tree?" >&2
    exit 1
fi
echo "photon:   $PHOTON"
echo "siblings: $ROOT"

# The closure: walk photon's Cargo.toml path deps transitively, keeping anything that resolves inside the sibling root.
# Pure shell BY RULE — the no-python gate forbids python in build scripts (this tree is Rust-only), and it caught this very script on its first run.
closure() {
    local todo="photon" seen="" cur dir rel abs name
    while [ -n "$todo" ]; do
        cur="${todo%% *}"
        case "$todo" in *" "*) todo="${todo#* }" ;; *) todo="" ;; esac
        case " $seen " in *" $cur "*) continue ;; esac
        seen="$seen $cur"
        dir="$ROOT/$cur"
        [ -f "$dir/Cargo.toml" ] || continue
        # Every `path = "..."` value, relative to THIS repo, resolved and kept only if it lands under the sibling root.
        for rel in $(sed -n 's/.*path[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$dir/Cargo.toml"); do
            abs="$(cd "$dir/$rel" 2>/dev/null && pwd)" || continue
            case "$abs" in
                "$ROOT"/*) name="${abs#"$ROOT"/}"; name="${name%%/*}" ;;
                *) continue ;;
            esac
            case " $seen $todo " in *" $name "*) ;; *) todo="$todo $name" ;; esac
        done
    done
    # Sorted, single-spaced.
    echo $(printf '%s\n' $seen | sort)
}
REPOS="$(closure)"

echo "closure:  $REPOS"
echo

fail=0
dirty=""
for r in $REPOS; do
    d="$ROOT/$r"
    [ -d "$d/.git" ] || { printf '  %-12s SKIP   not a git checkout: %s\n' "$r" "$d"; continue; }

    # Detached HEAD or no tracking branch = a deliberate pin (winit-patched's vanilla checkout is the archetype). Never yank those.
    branch=$(git -C "$d" symbolic-ref --quiet --short HEAD 2>/dev/null) || {
        printf '  %-12s SKIP   detached HEAD (pinned)\n' "$r"; continue; }
    git -C "$d" rev-parse --abbrev-ref "@{upstream}" >/dev/null 2>&1 || {
        printf '  %-12s SKIP   no upstream for %s\n' "$r" "$branch"; continue; }

    if ! git -C "$d" fetch --quiet origin 2>/dev/null; then
        printf '  %-12s FAIL   fetch failed (network?): %s\n' "$r" "$d"; fail=1; continue
    fi

    local_sha=$(git -C "$d" rev-parse HEAD)
    remote_sha=$(git -C "$d" rev-parse "@{upstream}")
    if [ "$local_sha" = "$remote_sha" ]; then
        state="current"
    elif git -C "$d" merge-base --is-ancestor HEAD "@{upstream}" 2>/dev/null; then
        if git -C "$d" merge --ff-only --quiet "@{upstream}" 2>/dev/null; then
            state="PULLED  $(git -C "$d" log --oneline -1 | cut -c1-52)"
        else
            printf '  %-12s FAIL   ff-only merge refused (uncommitted changes in the way?): %s\n' "$r" "$d"; fail=1; continue
        fi
    elif git -C "$d" merge-base --is-ancestor "@{upstream}" HEAD 2>/dev/null; then
        state="ahead — you have unpushed commits"
    else
        ahead=$(git -C "$d" rev-list --count "@{upstream}..HEAD" 2>/dev/null || echo '?')
        behind=$(git -C "$d" rev-list --count "HEAD..@{upstream}" 2>/dev/null || echo '?')
        printf '  %-12s FAIL   DIVERGED from %s: %s ahead, %s behind\n' "$r" "$branch" "$ahead" "$behind"
        printf '               %s\n' "$d"
        printf '               local-only commits:\n'
        git -C "$d" log --oneline "@{upstream}..HEAD" 2>/dev/null | sed 's/^/                 /'
        printf '               fix: git -C %s pull --rebase\n' "$d"
        fail=1; continue
    fi

    # Report a dirty tree but never touch it: uncommitted work is the human's call, and a deploy off a dirty tree ships something no commit describes.
    if [ -n "$(git -C "$d" status --porcelain 2>/dev/null)" ]; then
        dirty="$dirty $r"
        state="$state  [DIRTY]"
    fi
    printf '  %-12s %s\n' "$r" "$state"
done

echo
if [ -n "$dirty" ]; then
    echo "WARNING: uncommitted changes in:$dirty"
    for r in $dirty; do printf '         %s\n' "$ROOT/$r"; done
    echo "         a deploy from a dirty tree ships bits no commit can reproduce — commit or stash first."
    fail=1
fi

if [ "$fail" -ne 0 ]; then
    echo "NOT deploy-ready — resolve the above first."
    exit 1
fi
echo "all ${REPOS// /, } current, clean, and fast-forwarded — deploy-ready"
echo "completed $(date '+%F %T')"
