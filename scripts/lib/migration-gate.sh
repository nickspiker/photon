# Sourced, not executed. The migration expiry ratchet: a compatibility branch must name the day it dies, and the BUILD deletes it — not a memory, not a log line someone has to notice.
#
# Why this exists: a migration's removal condition is always "every device has read once", which is real but unobservable from a build. Left to prose (`// delete this once…`) the branch becomes permanent, because nothing ever forces the conversation. Same failure the VSF gate exists for: rules unenforced by tooling demonstrably did not survive.
#
# CONTRACT. Any temporary compatibility path carries, in a comment on its own line:
#
#     // MIGRATION-EXPIRES: 2027-02-01 — <one line: what it does, and what makes it safe to delete>
#
# The expiry is a CALENDAR DATE, never a release number (Nick 2026-11-03: a version ships about every day, and a few machines go a month without updating, so "thirty versions" is a month of wall time and a laggard jumps the pin and is stranded).
# Pick the date from how long a device can plausibly stay dark and still matter: three months is the floor.
# While today is before the date the build is silent. On the date and after, the build FAILS with every file and line, and the only ways forward are to delete the block or to make a deliberate, argued decision to move the date. Both are fine. Forgetting is not on the list.
# The date is read ONCE, here, on the build machine; the client never learns a date existed (edges, not timers).
# The scan covers photon and every sibling path-dep crate checked out beside it, because a pin in vsf or tohu strands a photon client exactly as a pin in photon does.
# MIGRATION_GATE_TODAY=YYYY-MM-DD overrides the clock for a dry run of sweep day.

migration_gate() {
    local root
    root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd) || return 1
    if [ ! -d "$root/src" ]; then
        echo "MIGRATION GATE: cannot locate src/ from ${BASH_SOURCE[0]} — refusing to pass silently." >&2
        return 1
    fi

    local today
    today="${MIGRATION_GATE_TODAY:-$(date +%F)}"
    if ! printf '%s' "$today" | grep -qE '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'; then
        echo "MIGRATION GATE: could not read today's date — refusing to pass silently." >&2
        return 1
    fi

    # photon's own tree plus every sibling path-dep crate present beside it.
    local roots="$root/src $root/android/app/src"
    local sib
    for sib in vsf fgtw tohu kete manifestus rarangi limbus opsin fluor chirp ihi nunc voca spirix; do
        [ -d "$root/../$sib/src" ] && roots="$roots $root/../$sib/src"
    done

    local fail=0 hit file line expires
    while IFS= read -r hit; do
        [ -z "$hit" ] && continue
        file="${hit%%:*}"
        file="${file#"$root"/}"; file="${file#../}"
        line=$(printf '%s' "$hit" | cut -d: -f2)
        # `MIGRATION-EXPIRES: 2027-02-01` → 2027-02-01
        expires=$(printf '%s' "$hit" | sed -E 's/.*MIGRATION-EXPIRES:[[:space:]]*([0-9]{4}-[0-9]{2}-[0-9]{2}).*/\1/')
        if ! printf '%s' "$expires" | grep -qE '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'; then
            echo "MIGRATION GATE: ${file#"$root"/}:$line has a malformed MIGRATION-EXPIRES marker (want 'MIGRATION-EXPIRES: 2027-02-01 — why')." >&2
            fail=1
            continue
        fi
        # ISO dates order as strings.
        if ! [[ "$today" < "$expires" ]]; then
            echo "MIGRATION GATE: ${file#"$root"/}:$line — this compatibility branch expired on $expires and today is $today." >&2
            fail=1
        fi
    done < <(grep -rn "MIGRATION-EXPIRES:" $roots --include=*.rs --include=*.kt 2>/dev/null | grep -v "/target/" || true)

    if [ "$fail" = "1" ]; then
        echo "MIGRATION GATE: build blocked — an expired compatibility branch is still in the tree." >&2
        echo "  Delete it. Its whole point was to be temporary, and the day it was waiting for has come." >&2
        echo "  If it genuinely still has work to do, move the date AND say why in the same commit — that is a decision, not an oversight." >&2
        return 1
    fi
}
