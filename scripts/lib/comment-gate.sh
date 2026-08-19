# Sourced, not executed. Wrapped-comment ratchet: hard-wrapped comment prose is banned in photon AND its path-dep fgtw (Rust: // ///) AND the build scripts (shell: #) — one line per thought, a sentence never continues onto the next comment line.
# A wrap = a comment line longer than 60 columns ending mid-clause (a word character) whose next line is the same comment marker continuing in lowercase. Long lines are CORRECT; continuations are the defect.
# The .sh scan is heredoc-aware: a `#` line inside a `<<EOF` body is content (or another language's comment), not a shell comment, so it is skipped.
# Baseline is ZERO in every tree. Do not add a baseline mechanism — fix the comment instead.

comment_gate() {
    local rs_roots rs_off sh_off offenders
    rs_roots="src"
    [ -d ../fgtw/src ] && rs_roots="$rs_roots ../fgtw/src"

    # Rust: // and /// markers.
    rs_off=$(find $rs_roots -name "*.rs" -not -path "*/target/*" | while read -r f; do
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

    # Shell: # marker (never the #! shebang), heredoc-aware.
    sh_off=$(find . -maxdepth 3 -name "*.sh" -not -path "*/node_modules/*" -not -path "*/target/*" 2>/dev/null | sort -u | while read -r f; do
        awk -v F="$f" '
            FNR == 1 { pm = ""; prev = ""; term = "" }
            {
                if (term != "") { tl = $0; sub(/^[[:space:]]*/, "", tl); if (tl == term) term = ""; pm = ""; prev = $0; next }
                if (match($0, /<<-?["'"'"']?[A-Za-z_][A-Za-z0-9_]*/)) { h = substr($0, RSTART, RLENGTH); sub(/^<<-?["'"'"']?/, "", h); term = h }
                m = ""
                if ($0 ~ /^[[:space:]]*#/ && $0 !~ /^[[:space:]]*#!/) m = "#"
                if (pm == "#" && m == "#") {
                    body = $0; sub(/^[[:space:]]*#+ ?/, "", body)
                    if (length(prev) > 60 && prev ~ /[A-Za-z]$/ && body ~ /^[a-z]/) print F ":" FNR - 1
                }
                pm = m; prev = $0
            }
        ' "$f"
    done)

    offenders=$(printf '%s\n%s\n' "$rs_off" "$sh_off" | grep -v '^$')
    if [ -n "$offenders" ]; then
        echo "COMMENT GATE: hard-wrapped comment prose (one line per thought, never wrapped):" >&2
        echo "$offenders" >&2
        echo "COMMENT GATE: build blocked — join each sentence onto one line." >&2
        return 1
    fi
    return 0
}

# NO-PYTHON ratchet: this is a Rust tree and ships no Python. A build script that shells out to python is
# the defect (the 2026-08 key-guard heredocs snuck in and were missed). Replace it with a Rust bitty-executable
# (rustc a std-only file, or a src/bin/ cargo bin) — see tools/key-guard.rs and src/bin/arch-gate.rs. Comment
# lines are exempt so this doc line and this history can name it.
no_python_gate() {
    local hits
    # comment-gate.sh itself is exempt: it is the one file whose job is to NAME python in order to ban it.
    hits=$(find . -maxdepth 3 -name "*.sh" -not -path "*/node_modules/*" -not -path "*/target/*" -not -name comment-gate.sh 2>/dev/null | sort -u | while read -r f; do
        awk -v F="$f" '{ l = $0; sub(/^[[:space:]]*/, "", l); if (l ~ /^#/) next; if ($0 ~ /(^|[^A-Za-z0-9_])python[0-9]?([^A-Za-z0-9_]|$)/) print F ":" FNR ": " $0 }' "$f"
    done)
    if [ -n "$hits" ]; then
        echo "NO-PYTHON GATE: Python in a build script — this tree is Rust-only. Replace it with a Rust bitty-executable:" >&2
        echo "$hits" >&2
        return 1
    fi
    return 0
}
