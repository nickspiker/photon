# Sourced, not executed. The migration expiry ratchet: a compatibility branch must name the release it dies in, and the BUILD deletes it — not a memory, not a log line someone has to notice.
#
# Why this exists: a migration's removal condition is always "every device has read once", which is real but unobservable from a build. Left to prose (`// delete this once…`) the branch becomes permanent, because nothing ever forces the conversation. Same failure the VSF gate exists for: rules unenforced by tooling demonstrably did not survive.
#
# CONTRACT. Any temporary compatibility path carries, in a comment on its own line:
#
#     // MIGRATION-EXPIRES: v56 — <one line: what it does, and what makes it safe to delete>
#
# `v56` is a RELEASE number, matching deploy.sh's counter (the MINOR in 0.<release>.<patch>). While the tree's release is below it the build is silent. The release it lands on, the build FAILS with the file and line, and the only ways forward are to delete the block or to make a deliberate,
# argued decision to move the number. Both are fine. Forgetting is not on the list.
#
# Pick an expiry from how long a device can plausibly stay dark and still matter — a few releases,
# not a few years. Photon ships a release every day or two; v+4 gives real devices weeks of runway.

migration_gate() {
    local root
    root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd) || return 1
    if [ ! -d "$root/src" ]; then
        echo "MIGRATION GATE: cannot locate src/ from ${BASH_SOURCE[0]} — refusing to pass silently." >&2
        return 1
    fi

    # The tree's release number is the MINOR (deploy.sh: 0.<release>.<patch>).
    local release
    release=$(grep -m1 '^version' "$root/Cargo.toml" | sed -E 's/.*"[0-9]+\.([0-9]+)\.[0-9]+".*/\1/')
    if ! [ "$release" -eq "$release" ] 2>/dev/null; then
        echo "MIGRATION GATE: could not read the release number from Cargo.toml — refusing to pass silently." >&2
        return 1
    fi

    local fail=0 hit file line expires
    while IFS= read -r hit; do
        [ -z "$hit" ] && continue
        file="${hit%%:*}"
        line=$(printf '%s' "$hit" | cut -d: -f2)
        # `MIGRATION-EXPIRES: v56` → 56
        expires=$(printf '%s' "$hit" | sed -E 's/.*MIGRATION-EXPIRES:[[:space:]]*v?([0-9]+).*/\1/')
        if ! [ "$expires" -eq "$expires" ] 2>/dev/null; then
            echo "MIGRATION GATE: ${file#"$root"/}:$line has a malformed MIGRATION-EXPIRES marker (want 'MIGRATION-EXPIRES: v56 — why')." >&2
            fail=1
            continue
        fi
        if [ "$release" -ge "$expires" ]; then
            echo "MIGRATION GATE: ${file#"$root"/}:$line — this compatibility branch expired at v$expires and the tree is at v$release." >&2
            echo "  Delete it. Its whole point was to be temporary, and the release it was waiting for has shipped." >&2
            echo "  If it genuinely still has work to do, move the marker AND say why in the same commit — that is a decision, not an oversight." >&2
            fail=1
        fi
    done < <(grep -rn "MIGRATION-EXPIRES:" "$root/src" --include=*.rs 2>/dev/null || true)

    if [ "$fail" = "1" ]; then
        echo "MIGRATION GATE: build blocked — an expired compatibility branch is still in the tree." >&2
        return 1
    fi
}
