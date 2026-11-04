# Sourced, not executed. The text-hash ratchet: a hash computed over a string's own bytes may not GROW anywhere in photon or its sibling crates.
#
# Why this exists (Nick 2026-11-03, security audit LIB-14 and the class behind it): an identity hashed from the bytes a human typed, rather than from the bytes that go on the wire, is a second derivation of one identity, and it agrees with the first only for ASCII.
# It has bitten four times: the handle folds that squatted other people's slots, the content-string ident that broke on a marker rename, the provenance hash that scanned bodies, and the rarangi row address hashed from the raw key while its catalog held the NFC wire form.
# The rule: human text reaches a hash by ONE road, the VSF x encoder (`VsfType::x(text).flatten()`, the pipeline the handle hash uses), never `text.as_bytes()`.
# Hashes over keys, ids and other byte values are not text and are not the target; the pattern below cannot tell a `String` from a `blake3::Hash`, so the baselines hold the audited counts and the human reads each new hit.
#
# PATTERN: a hash or KDF call whose argument chain is `.as_bytes()` on one line: `blake3::hash(x.as_bytes())`, `hasher.update(x.as_bytes())`, `derive_key(ctx, x.as_bytes())`, `spaghettify(x.as_bytes())`.
# Baselines are the audited counts as of 2026-11-03. Shrink them as sites are converted; NEVER raise one without a documented reason, and a file not listed is held at ZERO.

text_hash_gate() {
    local root
    root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd) || return 1
    if [ ! -d "$root/src" ]; then
        echo "TEXT-HASH GATE: cannot locate src/ from ${BASH_SOURCE[0]} — refusing to pass silently." >&2
        return 1
    fi
    local parent
    parent=$(cd "$root/.." && pwd)

    # file → audited count. Every line here is a site a human read on 2026-11-03 and judged to be bytes, not text, or debt to convert.
    local baseline="
photon/src/storage/cloud.rs 1
photon/src/crypto/clutch.rs 3
photon/src/types/device.rs 1
photon/src/types/conversation.rs 1
photon/src/ui/photon_app/peers.rs 1
photon/src/types/contact.rs 1
photon/src/network/traverse/punch.rs 1
photon/src/ui/avatar.rs 1
photon/src/types/friendship.rs 1
photon/src/network/status.rs 2
photon/src/ui/photon_app/ideas.rs 1
photon/src/ui/photon_app/settings.rs 1
photon/src/ui/photon_app.rs 1
photon/src/ui/photon_app/status.rs 1
fgtw/src/scoped_blob.rs 1
vsf/src/visual/mod.rs 2
chirp/src/lib.rs 1
"
    local roots="$root/src"
    local sib
    for sib in vsf fgtw tohu kete manifestus rarangi limbus opsin fluor chirp ihi nunc voca spirix; do
        [ -d "$parent/$sib/src" ] && roots="$roots $parent/$sib/src"
    done

    local fail=0 f n rel allowed
    while IFS=: read -r f n; do
        [ -z "$f" ] && continue
        rel="${f#"$parent"/}"
        allowed=$(printf '%s\n' "$baseline" | awk -v r="$rel" '$1 == r { print $2 }')
        [ -z "$allowed" ] && allowed=0
        if [ "$n" -gt "$allowed" ]; then
            echo "TEXT-HASH GATE: $rel has $n hash-over-string-bytes site(s), baseline $allowed:" >&2
            grep -n -E "(blake3::hash|derive_key|\.update|spaghettify)\([^)]*\.as_bytes\(\)" "$f" | sed 's/^/    /' >&2
            fail=1
        fi
    done < <(grep -rc -E "(blake3::hash|derive_key|\.update|spaghettify)\([^)]*\.as_bytes\(\)" $roots --include=*.rs 2>/dev/null | grep -v "/target/" | grep -v ":0$")

    if [ "$fail" = "1" ]; then
        echo "TEXT-HASH GATE: build blocked — a hash over a string's bytes was added. Human text reaches a hash through VsfType::x(text).flatten(); bytes that are not text get a comment saying what they are and the baseline moves in the same commit, with the reason." >&2
        return 1
    fi
}
