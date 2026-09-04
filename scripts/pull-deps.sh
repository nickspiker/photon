#!/bin/bash
# Fast-forward photon AND every sibling repo it actually depends on, so a build/deploy can't mix a fresh photon with a stale sibling.
#
# WHY THIS EXISTS: `for d in ~/Code/*/` walks ~85 repos, most of which photon never touches — slow, and it pulls unrelated work you may not want moving. The dependency CLOSURE is 12 repos, and it is derived here from the Cargo.toml `path = "..."` graph rather than hardcoded, so it stays right when deps change. Cargo's git deps (the [patch.crates-io] winit/softbuffer/audiopus_sys forks) live in ~/.cargo/git and are cargo's problem, not this script's — the local winit-patched checkout is deliberately NOT in the graph.
#
# A stale sibling is the "missing fgtw/fluor symbol" class: photon compiles against a sibling that lacks the symbol its new code calls, producing dozens of phantom errors that look like photon's fault. Fast-forward everything together or nothing.
#
# Exit status is deploy-gateable: 0 = every repo current and clean, non-zero = at least one repo needs a human.

set -u
cd "$(dirname "$0")/../.."   # ~/Code — the parent that holds every sibling
ROOT="$PWD"

# The closure: walk photon's Cargo.toml path deps transitively, keeping anything that resolves inside ~/Code.
REPOS=$(python3 - "$ROOT" <<'PY'
import re, os, sys
root = sys.argv[1]
seen, stack = set(), ['photon']
while stack:
    r = stack.pop()
    if r in seen:
        continue
    seen.add(r)
    ct = os.path.join(root, r, 'Cargo.toml')
    if not os.path.exists(ct):
        continue
    for m in re.finditer(r'path\s*=\s*"([^"]+)"', open(ct).read()):
        p = os.path.normpath(os.path.join(root, r, m.group(1)))
        if p.startswith(root + os.sep):
            n = os.path.relpath(p, root).split(os.sep)[0]
            if n not in seen:
                stack.append(n)
print(' '.join(sorted(seen)))
PY
)

echo "dependency closure: $REPOS"
echo

fail=0
dirty=""
for r in $REPOS; do
    d="$ROOT/$r"
    [ -d "$d/.git" ] || { printf '  %-12s SKIP   not a git repo\n' "$r"; continue; }

    # Detached HEAD or no tracking branch = a deliberate pin (winit-patched's vanilla checkout is the archetype). Never yank those.
    branch=$(git -C "$d" symbolic-ref --quiet --short HEAD 2>/dev/null) || {
        printf '  %-12s SKIP   detached HEAD (pinned)\n' "$r"; continue; }
    git -C "$d" rev-parse --abbrev-ref "@{upstream}" >/dev/null 2>&1 || {
        printf '  %-12s SKIP   no upstream for %s\n' "$r" "$branch"; continue; }

    if ! git -C "$d" fetch --quiet origin 2>/dev/null; then
        printf '  %-12s FAIL   fetch failed (network?)\n' "$r"; fail=1; continue
    fi

    local_sha=$(git -C "$d" rev-parse HEAD)
    remote_sha=$(git -C "$d" rev-parse "@{upstream}")
    if [ "$local_sha" = "$remote_sha" ]; then
        state="current"
    elif git -C "$d" merge-base --is-ancestor HEAD "@{upstream}" 2>/dev/null; then
        if git -C "$d" merge --ff-only --quiet "@{upstream}" 2>/dev/null; then
            state="PULLED  $(git -C "$d" log --oneline -1 | cut -c1-52)"
        else
            printf '  %-12s FAIL   ff-only merge refused (uncommitted changes in the way?)\n' "$r"; fail=1; continue
        fi
    elif git -C "$d" merge-base --is-ancestor "@{upstream}" HEAD 2>/dev/null; then
        state="ahead — you have unpushed commits"
    else
        printf '  %-12s FAIL   DIVERGED from %s — history was rewritten or both sides moved\n' "$r" "$branch"; fail=1; continue
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
    echo "         a deploy from a dirty tree ships bits no commit can reproduce — commit or stash first."
    fail=1
fi

if [ "$fail" -ne 0 ]; then
    echo "NOT deploy-ready — resolve the above first."
    exit 1
fi
echo "all ${REPOS// /, } current, clean, and fast-forwarded — deploy-ready"
echo "completed $(date '+%F %T')"
