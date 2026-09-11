# Sourced, not executed. ATTRIBUTION RATCHET (Nick 2026-09-11: "No co-authoring! It's Built with!"). Claude is a tool, not an author: a commit says who BUILT it with what, never who co-wrote it.
# The banned trailer keeps coming back because some hosts inject it as a default instruction mid-session, so the rule lives here where the build can enforce it instead of in a memory a session may not have read.
# Scope: the commits this tree has that the remote does not — the ones still cheap to amend. History that is already public is a separate, deliberate decision (see docs/attribution.md).

trailer_gate() {
    local range offenders
    # Nothing to check without an upstream (a fresh clone mid-rebase, a detached head): stay quiet rather than guess a range.
    git rev-parse --abbrev-ref --symbolic-full-name '@{u}' >/dev/null 2>&1 || return 0
    range="@{u}..HEAD"
    # A TRAILER, not a mention: the line must START with it, or this gate fails on the very commit that documents it.
    offenders=$(git log "$range" --format="%H" --grep="^Co-Authored-By:" --extended-regexp 2>/dev/null | grep -c . || true)
    if [ "${offenders:-0}" != "0" ]; then
        echo "TRAILER GATE: unpushed commit(s) carry a Co-Authored-By trailer — Claude is a tool, not an author:"
        git log "$range" --grep="^Co-Authored-By:" --extended-regexp --format="  %h %s" | head -20
        echo "TRAILER GATE: use \"Built with Claude <version>\" instead — amend or rebase these, then build again."
        return 1
    fi
}
