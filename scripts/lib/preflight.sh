# Sourced, not executed. The cheap SOURCE-LEVEL ratchets — pure text/scan checks plus the tiny arch probe, all sub-second, NO build required. Run these FIRST at every build/publish entry point, before the version bump and before the multi-minute cross-compile, so a wrapped comment / raw-parse site / expired-migration / python-shellout / risky-instruction fails instantly instead of after Android and macOS have already cross-built and published (the 2026-08-19 waste: two full publishes completed before the desktop step's gate ever read a comment).
#
# Each gate `return 1`s on failure, so `|| exit 1` is what actually aborts the caller.
preflight_gates() {
    local d
    d="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    source "$d/comment-gate.sh"
    comment_gate || exit 1
    no_python_gate || exit 1
    source "$d/vsf-gate.sh"
    vsf_gate || exit 1
    source "$d/migration-gate.sh"
    migration_gate || exit 1
    source "$d/arch-gate.sh"
    arch_gate || exit 1
}
