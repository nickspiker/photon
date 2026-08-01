# Sourced, not executed. Wrapped-comment ratchet: hard-wrapped comment prose is banned in photon AND its path-dep fgtw (one line per thought — a sentence never continues onto the next comment line).
# A wrap = a comment line longer than 60 columns ending mid-clause (a word character) whose next line is the same comment marker continuing in lowercase. Long lines are CORRECT; continuations are the defect.
# Baseline is ZERO in both trees. Do not add a baseline mechanism — fix the comment instead.

comment_gate() {
    local roots offenders
    roots="src"
    [ -d ../fgtw/src ] && roots="$roots ../fgtw/src"
    offenders=$(find $roots -name "*.rs" -not -path "*/target/*" | while read -r f; do
        awk -v F="$f" '
            FNR == 1 { pm = ""; prev = "" }
            {
                m = ""
                if ($0 ~ /^[[:space:]]*\/\//) {
                    t = $0; sub(/^[[:space:]]*/, "", t)
                    m = (substr(t, 1, 3) == "///") ? "///" : "//"
                }
                if (pm != "" && m == pm) {
                    body = $0; sub(/^[[:space:]]*\/+!?\/* ?/, "", body)
                    if (length(prev) > 60 && prev ~ /[A-Za-z]$/ && body ~ /^[a-z]/) print F ":" FNR - 1
                }
                pm = m; prev = $0
            }
        ' "$f"
    done)
    if [ -n "$offenders" ]; then
        echo "COMMENT GATE: hard-wrapped comment prose (one line per thought, never wrapped):" >&2
        echo "$offenders" >&2
        echo "COMMENT GATE: build blocked — join each sentence onto one line." >&2
        return 1
    fi
    return 0
}
