#!/bin/bash
# Hermetic test for installers/photon-launch.sh — the resilient launcher. Fakes two "photon" copies (good / bad-sig / hangs / missing) via env-overridden paths and asserts the shim picks the right one, falls back, times out, and refuses when all are bad. No real binary, no network; runs in seconds.
set -u

SHIM="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../installers" && pwd)/photon-launch.sh"

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); echo "  ok   - $1"; }
bad() { FAIL=$((FAIL+1)); echo "  FAIL - $1"; }

ROOT="$(mktemp -d)"
trap 'rm -rf "$ROOT"' EXIT

# Fake photon copies. Each answers `verify` per its role, and on a normal launch prints a tag + whether the shim handed it PHOTON_LAUNCH_VERIFIED — so we can assert both WHICH copy ran and that the one-validation handoff happened.
make_copy() { # <path> <role: good|bad|hang>
    local p="$1" role="$2"
    cat > "$p" <<EOF
#!/bin/sh
if [ "\$1" = "verify" ]; then
    case "$role" in
        good) exit 0 ;;
        bad)  exit 1 ;;
        hang) sleep 30 ;;
    esac
fi
echo "RAN=$role VERIFIED=\${PHOTON_LAUNCH_VERIFIED:-unset}"
EOF
    chmod +x "$p"
}

run_shim() { # A_path B_path  -> stdout of whichever copy ran
    PHOTON_COPY_A="$1" PHOTON_COPY_B="$2" PHOTON_VERIFY_TIMEOUT="${3:-8}" sh "$SHIM" 2>/dev/null
}

GOOD="$ROOT/good"; make_copy "$GOOD" good
GOOD2="$ROOT/good2"; make_copy "$GOOD2" good
BAD="$ROOT/bad";  make_copy "$BAD" bad
HANG="$ROOT/hang"; make_copy "$HANG" hang
MISSING="$ROOT/nope"   # never created

echo "== resilient launch shim =="

out="$(run_shim "$GOOD" "$GOOD2")"
[ "$out" = "RAN=good VERIFIED=1" ] && ok "A good → runs A, and handoff set PHOTON_LAUNCH_VERIFIED" || bad "A good [$out]"

out="$(run_shim "$BAD" "$GOOD")"
[ "$out" = "RAN=good VERIFIED=1" ] && ok "A bad-sig → falls back to B" || bad "bad→B [$out]"

out="$(run_shim "$MISSING" "$GOOD")"
[ "$out" = "RAN=good VERIFIED=1" ] && ok "A missing → skips, runs B" || bad "missing→B [$out]"

# hang: A sleeps forever on verify; a 2s timeout must give up and fall thru to B. (Bounds the test at ~2s, proves the timeout path.)
start=$SECONDS
out="$(run_shim "$HANG" "$GOOD" 2)"
elapsed=$((SECONDS - start))
{ [ "$out" = "RAN=good VERIFIED=1" ] && [ "$elapsed" -lt 8 ]; } && ok "A hangs → timeout, falls back to B (${elapsed}s)" || bad "hang→B [$out] ${elapsed}s"

rc=0; PHOTON_COPY_A="$BAD" PHOTON_COPY_B="$BAD" sh "$SHIM" >/dev/null 2>&1 || rc=$?
[ "$rc" -ne 0 ] && ok "both bad → non-zero exit ($rc)" || bad "both bad should fail, got $rc"

rc=0; PHOTON_COPY_A="$MISSING" PHOTON_COPY_B="$MISSING" sh "$SHIM" >/dev/null 2>&1 || rc=$?
[ "$rc" -ne 0 ] && ok "both missing → non-zero exit ($rc)" || bad "both missing should fail, got $rc"

echo ""
echo "==== $PASS passed, $FAIL failed ===="
[ "$FAIL" -eq 0 ]
