# Sourced, not executed. CPU-FEATURE ratchet: fail the build if a dependency compiles CPU-specific code by default.
#
# Why this exists: pqcrypto-mlkem enables `neon` by default, which compiles PQClean's hand-written aarch64 Keccak assembly. That path uses the ARMv8.2 SHA3 extension (EOR3/RAX1/XAR/BCAX), which a Snapdragon 855 does not implement — so a Galaxy Note10+ died with `SIGILL, ILL_ILLOPC` in the clutch-keygen thread on EVERY ceremony while newer devices were fine (device crash buffer, 2026-08-01). Nothing in the build said a word: it compiled, linked, passed every test on the dev machine, and only the oldest phone in the fleet could tell.
#
# The rule: a crate may not silently opt us into instructions our oldest target lacks. Any dependency whose RESOLVED feature set contains an architecture feature must be listed here with a reason — normally `default-features = false` in Cargo.toml instead.
#
# What this cannot catch: a crate that emits arch-specific code with no feature flag at all, or one that dispatches at RUNTIME after checking (blake3 does this correctly, which is why it has never faulted). Runtime dispatch is always fine; it is the compile-time default that bites.

arch_gate() {
    local manifest="${1:-Cargo.toml}"

    if ! command -v cargo >/dev/null 2>&1; then
        return 0
    fi

    # Architecture features that change WHICH INSTRUCTIONS get emitted. Not an exhaustive list of every SIMD name in the ecosystem — it is the set that has actually shipped in this tree's dependency closure.
    local arch_features="avx|avx2|avx512|sse|sse2|sse3|sse41|sse42|neon|asm|simd|sha3|sha2ni|aesni|aes-ni|pclmul|vaes|sve"

    # BASELINE ALLOWLIST — the ratchet, same shape as the vsf gate: these crates already carry an arch feature AND have demonstrably run for weeks on the OLDEST device in the fleet (the Snapdragon 855 that SIGILLs on unsupported opcodes). That is empirical proof, not an assumption, and it is the only evidence that counts here.
    # Most are safe for one of two reasons: the feature names Rust's PORTABLE simd (a type-level choice, not an instruction set — zeroize, zerocopy, ppv-lite86, wasmparser, miniz_oxide, tiny-skia, fluor), or the crate DISPATCHES AT RUNTIME after checking (blake3, moxcms, zune-jpeg). keccak/sha3 gate their `asm` to x86_64.
    # The gate's job is to catch the NEXT arrival — a crate added tomorrow that quietly opts us into instructions an older phone lacks. Adding a name here means you checked; it is not a place to silence a failure.
    local allow="blake3|fluor|keccak|miniz_oxide|moxcms|ppv-lite86|sha3|tiny-skia|wasmparser|zerocopy|zeroize|zune-jpeg"

    local meta
    meta="$(cargo metadata --format-version 1 --manifest-path "$manifest" 2>/dev/null)"
    if [ -z "$meta" ]; then
        echo "ARCH GATE: cargo metadata produced nothing — refusing to pass vacuously (a gate that greens on its own failure is worse than none)."
        return 1
    fi

    # arch-gate bin (Rust, src/bin/arch-gate.rs) — no Python. Built on demand; metadata pipes in on stdin, arch features + allowlist as args.
    # Force a clean HOST linker for THIS build: android-env.sh sets the host to clang + `-fuse-ld=mold`, but inside the Android env `clang` is the NDK's clang-14 which rejects mold — so building the gate would fail and silently no-op the check on the ONE platform it matters most. `cc` with no extra flags links everywhere; the gate is host-only, so this never touches a cross target.
    local offenders
    offenders="$(printf '%s' "$meta" | env CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=cc CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS= cargo run -q --manifest-path "$manifest" --bin arch-gate -- "$arch_features" "$allow")"

    if [ -n "$offenders" ]; then
        echo "ARCH GATE: a dependency compiles CPU-specific code by default —"
        echo "$offenders" | sed 's/^/  /'
        echo "  This builds fine here and SIGILLs on any device without those instructions (a Snapdragon 855 already died this way, 2026-08-01)."
        echo "  Fix: default-features = false in Cargo.toml, or add the crate to the allowlist in scripts/lib/arch-gate.sh IF it dispatches at runtime."
        return 1
    fi
    return 0
}
