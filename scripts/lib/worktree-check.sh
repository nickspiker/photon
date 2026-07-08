# Sourced, not executed. The merge-back guard: surface git worktrees that hold work not on main, so
# an agent's isolated worktree can't silently rot for months, get forgotten, and get redone. This is a
# WARNING, never a build failure — it returns 0 always. Losing work is the failure; a noisy warning is
# the whole point.
#
# Why this exists: worktree isolation stops parallel agents clobbering each other's files, but it does
# NOTHING to merge the work back. An agent finishes (or its session dies), its worktree holds good work,
# nothing ever merges it, `git status` on main reads clean, and you assume it's done. Isolation defers
# file contention from write-time to merge-time — and abandoned worktrees rot at merge-time instead of
# forcing a resolution. This check makes "silently orphaned" into "you're told about it now."
#
# A worktree is flagged when EITHER is true:
#   - it has uncommitted changes (dirty tree), OR
#   - its HEAD has commits not reachable from main (unmerged commits).
# The main worktree itself (the one whose HEAD is checked out here) is never flagged for being dirty —
# your own in-progress edits are expected. Only OTHER worktrees are.

worktree_check() {
    # List worktrees as porcelain records; each starts with a `worktree <path>` line.
    local main_path
    main_path="$(git rev-parse --show-toplevel 2>/dev/null)" || return 0

    local flagged=0
    local wt="" head=""
    # `git worktree list --porcelain` emits blank-line-separated records: worktree/HEAD/branch/…
    while IFS= read -r line; do
        case "$line" in
            "worktree "*) wt="${line#worktree }" ;;
            "HEAD "*)     head="${line#HEAD }" ;;
            "")  # end of a record — evaluate it
                _worktree_check_one "$wt" "$head" "$main_path" && flagged=1
                wt=""; head="" ;;
        esac
    done < <(git worktree list --porcelain 2>/dev/null; echo)

    if [ "$flagged" = "1" ]; then
        echo "" >&2
        echo "  These worktrees hold work that is NOT on main. Merge it back or remove them:" >&2
        echo "    git worktree list                 # see them all" >&2
        echo "    git -C <path> log --oneline main..HEAD   # unmerged commits" >&2
        echo "    git -C <path> status              # uncommitted changes" >&2
        echo "    git worktree remove <path>        # discard once you've confirmed it's safe" >&2
    fi
    return 0
}

# Flag one worktree. Returns 0 (success = "flagged") if it warrants a warning, 1 otherwise — so the
# caller can OR the result. Skips the main working tree's own dirty state (expected in-progress edits).
_worktree_check_one() {
    local wt="$1" head="$2" main_path="$3"
    [ -z "$wt" ] && return 1
    local is_main=0
    [ "$wt" = "$main_path" ] && is_main=1

    # Unmerged commits: HEAD not reachable from main. (Applies to every worktree, including main —
    # though main's HEAD is by definition on main, so it never trips here.)
    local unmerged=0
    if [ -n "$head" ] && ! git merge-base --is-ancestor "$head" main 2>/dev/null; then
        unmerged=1
    fi

    # Dirty tree: uncommitted changes. Skipped for the main worktree (your live edits are fine).
    local dirty=0
    if [ "$is_main" = "0" ] && [ -n "$(git -C "$wt" status --porcelain 2>/dev/null)" ]; then
        dirty=1
    fi

    [ "$unmerged" = "0" ] && [ "$dirty" = "0" ] && return 1

    local why=""
    [ "$unmerged" = "1" ] && why="unmerged commits"
    [ "$dirty" = "1" ] && why="${why:+$why + }uncommitted changes"
    echo "WORKTREE GUARD: $wt — $why" >&2
    return 0
}
