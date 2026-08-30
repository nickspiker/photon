# Sourced, not executed. The hand-rolled-VSF ratchet: a file may not GROW new unverified VSF reads. Every read at a trust boundary goes thru `vsf::verification::read_verified` or `SectionBuilder::parse_document` (see docs/vsf-trust-remediation.md) — this gate makes the next violation a build failure instead of a code-review hope. Rules unenforced by tooling demonstrably did not survive (~490 violations).
#
# SCOPE (widened 2026-07-29): ALL of src/, not just src/network + src/ui/avatar.rs. The old scope was "network-facing files", which missed src/storage entirely — and that is where the cloud contacts blob shipped a bare section with no provenance hash, alongside the friendship-chain and history-page payloads that are sealed and pushed to every sibling. Disk is a trust boundary too, and a "storage" payload is one push_* away from being a network payload.
#
# PATTERN: `SectionBuilder::parse(` now counts alongside VsfHeader::decode / VsfSection::parse. It is the unverified read people actually reach for, it validates schema SHAPE only and never the provenance hash, and it was invisible to the old pattern — which is why none of those three bugs tripped this gate. The verified twin `parse_document` is deliberately not matched (the trailing `(` excludes it).
#
# Baselines are the audited counts as of the widening. Shrink them as files are converted; NEVER raise one without a documented reason — raising a baseline is the moment the disease returns.

vsf_gate() {
    # Resolve the repo root from THIS file, whatever the caller's cwd. The old relative `find` scanned nothing and PASSED SILENTLY when the cwd was not the repo root — a ratchet that fails open is not a ratchet.
    local root
    root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd) || return 1
    if [ ! -d "$root/src" ]; then
        echo "VSF GATE: cannot locate src/ from ${BASH_SOURCE[0]} — refusing to pass silently." >&2
        return 1
    fi

    local pattern='VsfHeader::decode|VsfSection::parse|SectionBuilder::parse\('
    # file=baseline pairs — audited remaining unverified-read counts (post-AEAD inner sections, messaging-rework-pending frames, dev-only inspectors, the local log reader/trimmer).
    local baselines=(
        "src/lib.rs=9"
        "src/network/fgtw/blob.rs=4"
        "src/network/fgtw/bootstrap.rs=1"
        "src/network/fgtw/protocol.rs=6"
        "src/network/fgtw/relay.rs=1"
        "src/network/inspect.rs=4"
        "src/network/peer_updates.rs=1"
        "src/network/pt/mod.rs=3"
        "src/network/status.rs=2"
        "src/network/wfd.rs=1"  # open_cred's inner `wfdcred` section parse runs AFTER kete AEAD open under the pair seal key — authenticated, the same post-AEAD-inner class as open_pong_sensitive (protocol.rs baseline)
        "src/network/udp.rs=2"
        "src/storage/contacts.rs=6"  # +1 (2026-08-02): load_legacy_conv_state re-parses the SAME own-vault contact record the audited apply_contact_state read covers — a self-terminating migration fallback, deleted with it; shrink back to 5 then.
        "src/storage/friendship.rs=1"
        "src/storage/settings.rs=1"
    )

    local fail=0
    local f rel count max entry

    while IFS= read -r f; do
        rel="${f#"$root"/}"
        # Comment lines are prose ABOUT the pattern — every conversion leaves a note naming what it replaced — not reads.
        count=$(grep -E "$pattern" "$f" 2>/dev/null | grep -cvE '^[[:space:]]*(//|///|//!)' || true)
        [ "${count:-0}" -eq 0 ] && continue
        max=0
        for entry in "${baselines[@]}"; do
            if [ "${entry%=*}" = "$rel" ]; then max="${entry#*=}"; break; fi
        done
        if [ "$count" -gt "$max" ]; then
            echo "VSF GATE: $rel has $count unverified VSF read(s) (baseline $max)." >&2
            echo "  New reads must go thru vsf::verification::read_verified or SectionBuilder::parse_document." >&2
            echo "  See docs/vsf-trust-remediation.md. Do not raise the baseline in scripts/lib/vsf-gate.sh without a documented reason." >&2
            fail=1
        fi
    done < <(find "$root/src" -name "*.rs")

    # WRITE side. A schema builder's `.encode()` emits the section body ALONE — no header, no creation time, no BLAKE3 provenance hash — so anything that seals or ships those bytes has produced something no reader can ever verify. That is the exact shape of the cloud-contacts, history-page and friendship-chain bugs, and no read-side pattern can catch it. A complete file is `VsfBuilder::new()…add_unboxed(name, section_bytes).build()`.
    # Deliberately narrow: flags `.encode()` whose result is fed to a seal/write/send in the SAME expression. Anything subtler stays a review question.
    local bare
    bare=$(grep -rnE '(encrypt_bytes|write_addr|send|post)\([^)]*\.encode\(\)' "$root/src" --include=*.rs 2>/dev/null | grep -vE '^[^:]*:[0-9]+:[[:space:]]*(//|///|//!)' || true)
    if [ -n "$bare" ]; then
        echo "VSF GATE: a bare section's .encode() bytes are sealed/written/sent directly — wrap in VsfBuilder first (COMPLETE FILES ONLY):" >&2
        echo "$bare" | sed "s|^$root/||; s/^/  /" >&2
        fail=1
    fi

    # DOMAIN VERSIONS. A hash-domain version is a literal BINARY numeral appended after the text — hash(x ‖ "PHOTON_X_v" ‖ [N u8]) — never an ASCII digit inside the byte string (repo convention 2026-08-01; the ASCII pattern breeds by being read and reproduced). Ratchet over the crypto paths: baseline = the frozen pre-convention domains (they flip only when their own layer takes a flag-day); NEVER raise one.
    local dpattern='b"[A-Za-z_ ]*_v[0-9]'
    local dbaselines=(
        "src/crypto/blind.rs=3"
        "src/crypto/clutch.rs=3"
    )
    while IFS= read -r f; do
        rel="${f#"$root"/}"
        count=$(grep -E "$dpattern" "$f" 2>/dev/null | grep -cvE '^[[:space:]]*(//|///|//!)' || true)
        [ "${count:-0}" -eq 0 ] && continue
        max=0
        for entry in "${dbaselines[@]}"; do
            if [ "${entry%=*}" = "$rel" ]; then max="${entry#*=}"; break; fi
        done
        if [ "$count" -gt "$max" ]; then
            echo "VSF GATE: $rel has $count ASCII-versioned hash domain(s) (baseline $max)." >&2
            echo "  Version bytes are binary numerals appended after the text: b\"PHOTON_X_v\\x01\", never b\"PHOTON_X_v1\"." >&2
            fail=1
        fi
    done < <(find "$root/src/crypto" "$root/src/types" -name "*.rs" 2>/dev/null)

    if [ "$fail" = "1" ]; then
        echo "VSF GATE: build blocked — hand-rolled VSF at a trust boundary." >&2
        return 1
    fi
}
