# Sourced, not executed. The hand-rolled-VSF ratchet: network-facing files may not GROW new raw `VsfHeader::decode` / `VsfSection::parse` call sites. Every read at a trust boundary goes thru `vsf::verification::read_verified` or `SectionBuilder::parse_document` (see docs/vsf-trust-remediation.md) — this gate makes the next violation a build failure instead of a code-review hope. Rules unenforced by tooling demonstrably did not survive (~490 violations).
#
# Baselines are the audited counts at the time the verified-read conversion landed (2026-07-06). Shrink them as files are converted; NEVER raise one without a documented reason — raising a baseline is the moment the disease returns.

vsf_gate() {
    local pattern='VsfHeader::decode|VsfSection::parse'
    # file=baseline pairs — the audited remaining raw-parse counts (post-AEAD inner sections, messaging-rework-pending frames, dev-only inspectors).
    local baselines=(
        "src/network/peer_updates.rs=3"
        "src/network/inspect.rs=4"
        "src/network/udp.rs=2"
        "src/network/status.rs=3"
        "src/network/fgtw/blob.rs=4"
        "src/network/fgtw/bootstrap.rs=1"
        "src/network/fgtw/protocol.rs=8"
        "src/network/fgtw/relay.rs=4"
        "src/network/pt/mod.rs=3"
    )

    local fail=0

    while IFS= read -r f; do
        local count
        count=$(grep -cE "$pattern" "$f" 2>/dev/null || true)
        [ "$count" = "0" ] && continue
        local max=0
        for entry in "${baselines[@]}"; do
            if [ "${entry%=*}" = "$f" ]; then max="${entry#*=}"; break; fi
        done
        if [ "$count" -gt "$max" ]; then
            echo "VSF GATE: $f has $count raw VsfHeader::decode/VsfSection::parse sites (baseline $max)." >&2
            echo "  New network-facing reads must go thru vsf::verification::read_verified or SectionBuilder::parse_document." >&2
            echo "  See docs/vsf-trust-remediation.md. Do not raise the baseline in scripts/lib/vsf-gate.sh without a documented reason." >&2
            fail=1
        fi

    done < <(find src/network src/ui/avatar.rs -name "*.rs" 2>/dev/null)

    if [ "$fail" = "1" ]; then
        echo "VSF GATE: build blocked — hand-rolled VSF at a trust boundary." >&2
        return 1
    fi
}
