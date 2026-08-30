# Sourced, not executed. The SEAM tripwire: a photon commit that depends on a sibling's new API is worthless on every other machine until that sibling is PUSHED — and nothing used to check the seam (field 2026-08-30: photon fdf386f needed chirp's ring_from_hash, chirp sat committed-but-local on the other machine, this MacBook could not build HEAD at all). Walks every `path = "../…"` dep in Cargo.toml plus photon itself; any repo whose branch holds commits its remotes lack fails loud with the exact push command.
#
# sibling_push_check          → warn only (dev loop: local WIP siblings are legal mid-work, but you get told every build)
# sibling_push_check --strict → return 1 on any unpushed sibling (deploy preflight: state must not leave the machine un-pushable)
sibling_push_check() {
    local strict=""
    [ "$1" = "--strict" ] && strict=1
    local root
    root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
    local bad=0 r name ahead
    # Photon itself + every path-dep sibling named in Cargo.toml (deduped; ../keys is private-by-design and has no remote discipline — skip anything without a remote).
    local repos
    repos="$root $(grep -oE 'path = "\.\./[a-z0-9_-]+"' "$root/Cargo.toml" | sed -E 's|path = "\.\./([a-z0-9_-]+)"|'"$(dirname "$root")"'/\1|' | sort -u)"
    for r in $repos; do
        [ -d "$r/.git" ] || continue
        git -C "$r" remote | grep -q . || continue
        name="$(basename "$r")"
        ahead="$(git -C "$r" log --branches --not --remotes --oneline 2>/dev/null | wc -l | tr -d ' ')"
        if [ "$ahead" != "0" ]; then
            echo "SEAM: $name has $ahead commit(s) no remote holds — every other machine is blind to them. Push: git -C $r push" >&2
            bad=1
        fi
    done
    if [ "$bad" = "1" ] && [ -n "$strict" ]; then
        echo "SEAM: refusing — push the repo(s) above first (a deploy from un-pushed sibling state strands every other machine)." >&2
        return 1
    fi
    return 0
}
