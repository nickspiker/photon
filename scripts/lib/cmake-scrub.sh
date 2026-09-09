# Sourced, not executed. Divergent-CMake-cache scrub for vendored -sys crates (audiopus_sys and any future cmake dep).
#
# THE FAILURE (field 2026-08-31): a cached out/build configured with a DIFFERENT C compiler than the current run makes CMake declare "variables changed, cache deleted" and re-run configure DURING THE BUILD STEP — where the original command line's -DCMAKE_INSTALL_PREFIX no longer applies, so the prefix silently resets to /usr/local and the install step tries to write libopus.a into the system root. Unprivileged it dies on permissions (the lucky outcome); under root it would pollute /usr/local and still fail at link. The compiler diverges whenever the same target dir is built thru different scripts/env vintages (NDK bump, toolchain move).
#
# THE GUARD: before building, inspect every vendored-dep CMakeCache under the target tree; scrub any whose recorded CMAKE_C_COMPILER is missing from disk or fails the caller's expected-substring match. A scrubbed dir just re-configures fresh on the next build — always with the right prefix. Idempotent, silent when clean.

# scrub_divergent_cmake_caches <target-root> [expected-compiler-substring]
#   <target-root>  the cargo target dir subtree to inspect (e.g. target/aarch64-linux-android)
#   [substring]    when given, a cache whose compiler path does NOT contain it is divergent (e.g. the NDK root for Android builds); when omitted, only existence is checked (host builds — the system compiler path is stable, but a deleted custom toolchain still gets caught).
scrub_divergent_cmake_caches() {
    local root="$1" expect="${2:-}" cache cc dir
    [ -d "$root" ] || return 0
    while IFS= read -r cache; do
        cc="$(sed -n 's/^CMAKE_C_COMPILER:[^=]*=//p' "$cache" | head -1)"
        # CMake resolves CMAKE_C_COMPILER to the real clang binary, so the API-suffixed wrapper cmake-rs was handed survives only in CMAKE_ASM_COMPILER (2026-09-09: matching the wrapper against the C compiler alone scrubbed a healthy cache on every Android build). The expected substring matches EITHER.
        asm="$(sed -n 's/^CMAKE_ASM_COMPILER:[^=]*=//p' "$cache" | head -1)"
        prefix="$(sed -n 's/^CMAKE_INSTALL_PREFIX:[^=]*=//p' "$cache" | head -1)"
        dir="${cache%/out/build/CMakeCache.txt}"
        # A cache whose prefix ALREADY reads /usr/local has been thru the mid-build re-configure once (2026-09-09: the compiler matched, the prefix was gone, the install step hit Permission denied) — it is poison whatever its compiler says.
        if [ -z "$cc" ] || [ ! -e "$cc" ] || { [ -n "$expect" ] && [ "${cc#*"$expect"}" = "$cc" ] && [ "${asm#*"$expect"}" = "$asm" ]; } || [ "$prefix" = "/usr/local" ]; then
            echo "cmake-scrub: divergent cache (compiler: ${cc:-unrecorded}, prefix: ${prefix:-unrecorded}) — scrubbing ${dir#"$PWD"/} (a re-configure mid-build loses the install prefix and aims at /usr/local)"
            rm -rf "$dir"
        fi
    done < <(find "$root" -maxdepth 8 -path "*/build/*/out/build/CMakeCache.txt" 2>/dev/null)
    return 0
}
